//! The server list's intents, the accounts on its servers, and the loop that probes them and asks
//! who's online (launcher spec §3.3, §3.4; L8, L10; plan LP7). Every request runs with the lock
//! released, each probe in its own task, so one silent server holds up nothing else.
use std::sync::{Arc, Weak};
use std::time::Duration;

use fenji_auth::invite::InviteCode;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{debug, info};

use super::{
    App, Inner, IntentError, KeyChange, Live, Notice, Secret, State, Tab, now_ms, refused, words,
};
use crate::api::{ApiClient, ApiError, Session};
use crate::probe::{self, Probe, Status};
use crate::servers::{Added, parse_added};
use crate::settings::AddOutcome;

/// How often the active server is probed (L10).
const ACTIVE_EVERY: Duration = Duration::from_secs(15);
/// How often every server is probed: the rail always shows them all (L10; the redesign's R8).
const SWEEP_EVERY: Duration = Duration::from_secs(30);
/// How often the active server is asked who's online (spec §3.3).
const PLAYERS_EVERY: Duration = Duration::from_secs(15);
/// How often a server's "last seen" reaches the disk.
const SEEN_SAVED_EVERY: Duration = Duration::from_secs(60);

/// Every second, whatever is due. The loop ends with the last `App`.
pub(super) async fn run(inner: Weak<Inner>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        App { inner }.tick();
    }
}

/// Whether an answer means the session is over: it ended, or the host banned the account.
pub(super) fn session_over(e: &ApiError) -> bool {
    match e {
        ApiError::Banned { .. } => true,
        ApiError::Server { code, .. } => code == "bad_session",
        _ => false,
    }
}

impl App {
    /// The loop's work: the active server every 15 s (and at once at start), every server every
    /// 30 s, and who's online every 15 s.
    fn tick(&self) {
        let now = Instant::now();
        let due: Vec<u32> = {
            let mut st = self.inner.lock();
            let mut due = Vec::new();
            if let Some(id) = st.settings.active
                && st
                    .live
                    .get(&id)
                    .and_then(|l| l.probed_at)
                    .is_none_or(|at| now - at >= ACTIVE_EVERY)
            {
                due.push(id);
            }
            if st.swept_at.is_none_or(|at| now - at >= SWEEP_EVERY) {
                st.swept_at = Some(now);
                due.extend(st.settings.servers.iter().map(|s| s.id));
            }
            due
        };
        self.probe(due);
        self.list_players(false);
    }

    /// Probes each server not already being probed, each in its own task.
    fn probe(&self, ids: impl IntoIterator<Item = u32>) {
        let now = Instant::now();
        let started: Vec<(u32, Arc<ApiClient>)> = self.change(|st| {
            let State {
                settings,
                live: all,
                ..
            } = st;
            ids.into_iter()
                .filter_map(|id| {
                    let saved = settings.server(id)?;
                    let live = all.entry(id).or_default();
                    if live.probing {
                        return None;
                    }
                    live.probed_at = Some(now);
                    match live.client(saved) {
                        Ok(api) => {
                            live.probing = true;
                            Some((id, api))
                        }
                        Err(e) => {
                            debug!(server = id, error = %e, "can't probe this address");
                            live.status = Some(Status::Offline);
                            None
                        }
                    }
                })
                .collect()
        });
        for (id, api) in started {
            let app = self.clone();
            self.inner.rt.spawn(async move {
                let found = probe::probe(&api, &app.inner.cfg.pinned_build).await;
                app.probed(id, &api, found);
            });
        }
    }

    pub(super) fn probed(&self, id: u32, api: &Arc<ApiClient>, found: Probe) {
        let now = Instant::now();
        self.change(|st| {
            let Some(live) = st.live.get_mut(&id) else {
                return;
            };
            // A client since replaced (the player took a new key) answers for the old key.
            if !live.api.as_ref().is_some_and(|a| Arc::ptr_eq(a, api)) {
                return;
            }
            live.probing = false;
            debug!(server = id, status = ?found.status, ping_ms = ?found.ping_ms, "probed");
            live.status = Some(found.status);
            live.ping_ms = found.ping_ms;
            let Some(info) = found.info else {
                return;
            };
            let name = info.name.clone();
            live.info = Some(info);
            let save_seen = live
                .seen_saved_at
                .is_none_or(|at| now - at >= SEEN_SAVED_EVERY);
            if save_seen {
                live.seen_saved_at = Some(now);
            }
            let Some(saved) = st.settings.server_mut(id) else {
                return;
            };
            saved.last_seen_ms = Some(now_ms());
            let renamed = saved.name.as_ref() != Some(&name);
            if renamed {
                saved.name = Some(name);
            }
            if renamed || save_seen {
                self.save(st);
            }
        });
    }

    /// Asks the active server who's online, while signed in there on the Play or Servers tab:
    /// when due, or `now`.
    fn list_players(&self, now: bool) {
        let at = Instant::now();
        let job = {
            let mut st = self.inner.lock();
            let st = &mut *st;
            if !matches!(st.tab, Tab::Play | Tab::Servers) {
                return;
            }
            let Some(id) = st.settings.active else {
                return;
            };
            let Some(live) = st.live.get_mut(&id) else {
                return;
            };
            let (Some(session), Some(api)) = (live.session.clone(), live.api.clone()) else {
                return;
            };
            if live.listing || (!now && live.listed_at.is_some_and(|t| at - t < PLAYERS_EVERY)) {
                return;
            }
            live.listing = true;
            live.listed_at = Some(at);
            (id, api, session)
        };
        let (id, api, session) = job;
        let app = self.clone();
        self.inner.rt.spawn(async move {
            let answer = api.players(&session).await;
            app.listed(id, &session, answer);
        });
    }

    fn listed(&self, id: u32, session: &Session, answer: Result<Vec<String>, ApiError>) {
        self.change(|st| {
            let Some(live) = st.live.get_mut(&id) else {
                return;
            };
            live.listing = false;
            // Signed out, or in again, while it asked.
            if live.session.as_ref() != Some(session) {
                return;
            }
            match answer {
                Ok(names) => live.lobby = Some(names),
                Err(e) if session_over(&e) => {
                    info!(server = id, error = %e, "the session ended");
                    (live.session, live.username, live.lobby) = (None, None, None);
                    let text = words::api_error(&e, &st.name(id), st.host(id).as_deref(), None);
                    st.notice = Some(Notice { text, error: true });
                }
                Err(e) => debug!(server = id, error = %e, "couldn't ask who's online"),
            }
        });
    }

    /// The Add dialog (spec §3.3; plan LP7): an address or a link adds its server, makes it
    /// active and probes it at once; a link's invite waits to be redeemed; a code alone waits on
    /// the active server.
    pub(super) fn add_server(&self, text: &str) -> Result<(), IntentError> {
        let added = parse_added(text).map_err(|e| IntentError::Refused(e.to_string()))?;
        let (addr, invite) = match added {
            Added::Code(code) => {
                return self.change(|st| {
                    let id = st
                        .settings
                        .active
                        .ok_or_else(|| refused(words::NEED_LINK))?;
                    st.pending_invites.insert(id, code);
                    Ok(())
                });
            }
            Added::Address(addr) => (addr, None),
            Added::Invite(addr, code) => (addr, Some(code)),
        };
        let id = self.change(|st| {
            let outcome = st.settings.add_server(&addr);
            let id = outcome.id();
            match outcome {
                // A pasted link could come from anyone, so its key (and its invite) wait for the
                // player.
                AddOutcome::KeyDiffers { saved, .. } => {
                    st.key_change = addr.pin.map(|offered| KeyChange {
                        id,
                        offered,
                        had_key: saved.is_some(),
                        invite,
                    });
                }
                AddOutcome::New(_) | AddOutcome::Known(_) => {
                    if let Some(code) = invite {
                        st.pending_invites.insert(id, code);
                    }
                }
            }
            st.settings.active = Some(id);
            self.save(st);
            id
        });
        info!(server = id, origin = %addr.origin, "added a server");
        self.probe([id]);
        Ok(())
    }

    /// Forgets a server: its address, pin and username, and this run's session, which ends on the
    /// server too. The account itself stays (spec §3.3).
    pub(super) fn remove_server(&self, id: u32) {
        let ended = self.change(|st| {
            if !st.settings.remove_server(id) {
                return None;
            }
            info!(server = id, "removed a server");
            st.transient.remove(&id);
            st.pending_invites.remove(&id);
            if st.key_change.as_ref().is_some_and(|k| k.id == id) {
                st.key_change = None;
            }
            let live = st.live.remove(&id);
            self.save(st);
            let live = live?;
            Some((live.api?, live.session?))
        });
        if let Some((api, session)) = ended {
            self.inner.rt.spawn(async move {
                let _ = api.sign_out(session).await;
            });
        }
    }

    pub(super) fn select_server(&self, id: u32) {
        let known = self.change(|st| {
            let known = st.settings.set_active(id);
            if known {
                self.save(st);
            }
            known
        });
        if known {
            self.probe([id]);
            self.list_players(false);
        }
    }

    /// Every server, at once.
    pub(super) fn refresh(&self) {
        let ids: Vec<u32> = self.change(|st| {
            st.swept_at = Some(Instant::now());
            st.settings.servers.iter().map(|s| s.id).collect()
        });
        self.probe(ids);
    }

    pub(super) fn view_tab(&self, tab: Tab) {
        self.change(|st| st.tab = tab);
        if tab == Tab::Servers {
            self.refresh();
        }
        self.list_players(false);
    }

    /// The window gained focus: the active server, at once.
    pub(super) fn focus(&self, focused: bool) {
        let active = self.inner.lock().settings.active;
        if let (true, Some(id)) = (focused, active) {
            self.probe([id]);
        }
    }

    /// Makes an account with an invite (spec §3.4): the code given, or the pending invite's,
    /// on the pending invite's server or else the active one; then signs in.
    pub(super) async fn redeem(
        &self,
        code: Option<String>,
        username: String,
        password: Secret,
    ) -> Result<(), IntentError> {
        let (id, code, api) = {
            let mut st = self.inner.lock();
            // On the active server, whose Join this is: a typed code, or the invite waiting there.
            let id = st
                .settings
                .active
                .ok_or_else(|| refused(words::NEED_LINK))?;
            let code = match code {
                Some(text) => InviteCode::parse(&text).ok_or_else(|| refused(words::BAD_CODE))?,
                None => st
                    .pending_invites
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| refused(words::NEED_LINK))?,
            };
            if st.key_change.as_ref().is_some_and(|k| k.id == id) {
                return Err(IntentError::Refused(words::key_first(&st.name(id))));
            }
            let api = match st.api(id) {
                Some(Ok(api)) => api,
                Some(Err(e)) => return Err(st.refusal(id, &e, Some(&username))),
                None => return Err(refused(words::NEED_LINK)),
            };
            (id, code, api)
        };
        if let Err(e) = api.register(&code.display(), &username, &password.0).await {
            info!(server = id, error = %e, "the invite wasn't taken");
            self.probe([id]);
            return Err(self.refusal(id, &e, Some(&username)));
        }
        info!(server = id, %username, "made an account with an invite");
        // The invite is spent and the account exists, whatever the sign-in says. Only that
        // invite: one pasted while it ran still waits.
        self.change(|st| {
            if st.pending_invites.get(&id) == Some(&code) {
                st.pending_invites.remove(&id);
            }
            if let Some(saved) = st.settings.server_mut(id) {
                saved.username = Some(username.clone());
            }
            st.transient.remove(&id);
            self.save(st);
        });
        self.session_on(id, api, username, password, true, None)
            .await
    }

    /// Signs in on the active server (spec §3.4). With `remember`, the username is saved; without,
    /// the server leaves the disk now, and the list when the launcher closes.
    pub(super) async fn sign_in(
        &self,
        username: String,
        password: Secret,
        remember: bool,
    ) -> Result<(), IntentError> {
        let (id, api, waiting) = {
            let mut st = self.inner.lock();
            let id = st
                .settings
                .active
                .ok_or_else(|| refused(words::NO_SERVER))?;
            let api = match st.api(id) {
                Some(Ok(api)) => api,
                Some(Err(e)) => return Err(st.refusal(id, &e, Some(&username))),
                None => return Err(refused(words::NO_SERVER)),
            };
            let waiting = st.pending_invites.get(&id).cloned();
            (id, api, waiting)
        };
        self.session_on(id, api, username, password, remember, waiting)
            .await
    }

    /// Signs in and keeps the session in memory only; the password goes with this call. A sign-in
    /// with an account the player had drops `waiting`, the invite that was waiting there when it
    /// began.
    async fn session_on(
        &self,
        id: u32,
        api: Arc<ApiClient>,
        username: String,
        password: Secret,
        remember: bool,
        waiting: Option<InviteCode>,
    ) -> Result<(), IntentError> {
        let answer = api.sign_in(&username, &password.0).await;
        drop(password);
        let session = match answer {
            Ok(session) => session,
            Err(e) => {
                info!(server = id, %username, error = %e, "sign-in refused");
                self.probe([id]);
                return Err(self.refusal(id, &e, Some(&username)));
            }
        };
        let mut resume = false;
        let unkept = self.change(|st| {
            // Removed while it signed in: the session has nowhere to live.
            if st.settings.server(id).is_none() {
                return Some((session, None));
            }
            // The player took a new key meanwhile: a token from the old key never meets the new.
            let current = st.live.get(&id).and_then(|l| l.api.as_ref());
            if !current.is_some_and(|c| Arc::ptr_eq(c, &api)) {
                let words = words::key_changed_meanwhile(&st.name(id));
                return Some((session, Some(IntentError::Refused(words))));
            }
            if remember {
                st.transient.remove(&id);
                if let Some(saved) = st.settings.server_mut(id) {
                    saved.username = Some(username.clone());
                }
            } else {
                // Unticked, the server is forgotten when the launcher closes, whenever it was
                // added (spec §3.4; #348): it leaves the disk now, and the list at the close.
                st.transient.insert(id);
            }
            let live = st.live.entry(id).or_default();
            live.session = Some(session);
            live.username = Some(username.clone());
            live.lobby = None;
            // Play waited for this sign-in (plan LP8): it carries on below.
            resume = std::mem::replace(&mut st.need_sign_in, false);
            // Signed in with an account they had: the invite waiting there isn't needed.
            if let Some(code) = waiting
                && st.pending_invites.get(&id) == Some(&code)
            {
                st.pending_invites.remove(&id);
            }
            self.save(st);
            None
        });
        if let Some((session, refusal)) = unkept {
            let _ = api.sign_out(session).await;
            return refusal.map_or(Ok(()), Err);
        }
        info!(server = id, %username, "signed in");
        self.list_players(true);
        if resume {
            self.play_soon();
        }
        Ok(())
    }

    /// Ends the active server's session there too, and drops it.
    pub(super) async fn sign_out(&self) {
        let ended = self.change(|st| {
            let id = st.settings.active?;
            let live = st.live.get_mut(&id)?;
            let session = live.session.take()?;
            (live.username, live.lobby) = (None, None);
            Some((id, live.api.clone()?, session))
        });
        if let Some((id, api, session)) = ended {
            info!(server = id, "signed out");
            if let Err(e) = api.sign_out(session).await {
                debug!(server = id, error = %e, "the server didn't take the sign-out; the session ends on its own");
            }
        }
    }

    /// The player trusts the key the last link offered: it's saved, and the server is asked
    /// again with it.
    pub(super) fn use_new_key(&self, id: u32) {
        let taken = self.change(|st| {
            let Some(change) = st.key_change.take_if(|k| k.id == id) else {
                return false;
            };
            st.settings.set_pin(id, Some(change.offered));
            if let Some(live) = st.live.get_mut(&id) {
                // A session and a client from the old key never meet the new one.
                *live = Live {
                    info: live.info.take(),
                    ..Live::default()
                };
            }
            if let Some(code) = change.invite {
                st.pending_invites.insert(id, code);
            }
            self.save(st);
            true
        });
        if taken {
            info!(server = id, "the player took the server's new key");
            self.probe([id]);
        }
    }

    /// An answer's words, as a refusal.
    fn refusal(&self, id: u32, e: &ApiError, username: Option<&str>) -> IntentError {
        self.inner.lock().refusal(id, e, username)
    }
}
