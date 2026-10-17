//! Play (launcher spec §3.7; L2, L12; plan LP8): the client started through umu-run with a fresh
//! ticket in its environment, and followed until it ends. The ticket's pairs go to the client's
//! environment only. The launcher stays open while the game runs, and closing it leaves the game
//! running.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;
use tracing::{info, warn};

use super::client::or_cancelled;
use super::{
    App, CheckView, GameView, IntentError, JobView, LevelView, Notice, State, now_ms, refused,
    words,
};
use crate::api::ApiError;
use crate::launch::base_spec;
use crate::options::LaunchOptions;
use crate::probe::{self, Incompatible, Status};
use crate::settings::Os;
use crate::start::{self, UmuOptions};
use crate::{proton, tools};

/// An end this soon after the start means the game couldn't start, whatever its code (spec §3.12;
/// #332).
const STARTUP: Duration = Duration::from_secs(15);
pub(super) const UMU_LAUNCHER: &str = "umu-launcher";
/// The latest GE-Proton, which Play fetches before a start when a newer one is out (launcher spec
/// Amendment B, LB1).
pub(super) const GE_PROTON: &str = "GE-Proton";
/// How long `umu-run createprefix` may take: a first one fetches umu's runtime, and Proton too when
/// none is set, before it builds the prefix (#348). `words::PREFIX_TOO_LONG` says it.
const PREFIX_LIMIT: Duration = Duration::from_secs(60 * 60);
/// How long a stopped process group has to end before it's killed.
const GRACE: Duration = Duration::from_secs(5);
/// How often a start looks for Cancel start.
const LOOK_EVERY: Duration = Duration::from_millis(100);

impl App {
    /// Play on the active server: the refusals, then a sign-in if there's no session (LP8), the
    /// quick check (L11), and the start, which carries on in its own task.
    pub(super) async fn play(&self) -> Result<(), IntentError> {
        if self.inner.cfg.os != Os::Linux {
            return Err(refused(words::NOT_HERE));
        }
        // Saved only when they read, unless by hand.
        let options = crate::options::parse(&self.inner.lock().settings.launch_options)
            .map_err(|e| refused(&words::launch_options(&e)))?;
        let id = {
            let st = self.inner.lock();
            if playing(&st) {
                return Err(refused(words::RUNNING));
            }
            st.settings
                .active
                .ok_or_else(|| refused(words::NO_SERVER))?
        };
        let status = self.status_now(id).await;
        let signed_in = {
            let st = self.inner.lock();
            if let Some(refusal) = st.not_playable(id, status.as_ref()) {
                return Err(refusal);
            }
            if st.settings.client.is_none() {
                return Err(refused(words::NO_FOLDER));
            }
            st.live.get(&id).is_some_and(|l| l.session.is_some())
        };
        if !signed_in {
            self.change(|st| st.need_sign_in = true);
            return Ok(());
        }
        let client = match self.check_here(LevelView::Quick).await? {
            Some((CheckView::Verified { .. }, folder)) => folder,
            Some((CheckView::NeedsRepair { .. }, _)) => return Err(refused(words::NEEDS_REPAIR)),
            Some(_) => return Err(refused(words::NO_FOLDER)),
            // Stopped, or it failed: its notice says so.
            None => return Ok(()),
        };
        let prefix = self.inner.cfg.data_dir.join("prefix");
        self.change(|st| {
            if playing(st) {
                return Err(refused(words::RUNNING));
            }
            // From Starting on, the folder can't change (#348): the start launches the one checked.
            still_checked(st.settings.client.as_deref(), &client)?;
            st.game = GameView::Starting {
                first_start: !start::prefix_ready(&prefix),
            };
            st.start_cancelled = false;
            Ok(())
        })?;
        let app = self.clone();
        self.inner.rt.spawn(async move {
            let started = app.start_game(id, prefix, client, options).await;
            app.change(|st| {
                // Whatever else stopped the start meanwhile, the player's cancel is the news.
                let cancelled = std::mem::take(&mut st.start_cancelled);
                if let Err(text) = started {
                    st.game = GameView::Idle;
                    st.notice = Some(if cancelled {
                        Notice {
                            text: words::START_CANCELLED.into(),
                            error: false,
                        }
                    } else {
                        Notice { text, error: true }
                    });
                }
            });
        });
        Ok(())
    }

    /// Cancel start (#348): the start sees it within [`LOOK_EVERY`], ends umu-run's prefix run with
    /// everything it started, and goes back to Play. The game itself, once running, is the
    /// player's to close.
    pub(super) fn cancel_start(&self) {
        self.change(|st| {
            if matches!(st.game, GameView::Starting { .. }) {
                st.start_cancelled = true;
            }
        });
    }

    fn start_cancelled(&self) -> bool {
        self.inner.lock().start_cancelled
    }

    /// The start goes on unless it was cancelled.
    fn going_on(&self) -> Result<(), String> {
        if self.start_cancelled() {
            return Err(words::START_CANCELLED.into());
        }
        Ok(())
    }

    /// Play once a sign-in it waited for has come (LP8); a refusal becomes the notice.
    pub(super) fn play_soon(&self) {
        let app = self.clone();
        self.inner.rt.spawn(async move {
            if let Err(IntentError::Refused(text)) = app.play().await {
                app.note(text, true);
            }
        });
    }

    /// The server's state, asking it now if it hasn't answered yet.
    async fn status_now(&self, id: u32) -> Option<Status> {
        let api = {
            let mut st = self.inner.lock();
            if let Some(status) = st.live.get(&id).and_then(|l| l.status.clone()) {
                return Some(status);
            }
            st.api(id)?.ok()?
        };
        let found = probe::probe(&api, &self.inner.cfg.pinned_build).await;
        let status = found.status.clone();
        self.probed(id, &api, found);
        Some(status)
    }

    /// umu-run, the prefix, the ticket, the start, and the wait. An error is the notice's words.
    /// The launch options' variables go to making the prefix and to the game; their wrappers and
    /// arguments to the game alone.
    async fn start_game(
        &self,
        id: u32,
        prefix: PathBuf,
        client: PathBuf,
        options: LaunchOptions,
    ) -> Result<(), String> {
        let cfg = &self.inner.cfg;
        let umu_run = self.umu_run().await?;
        self.going_on()?;
        let proton = self.proton(&options).await;
        self.going_on()?;
        // The Proton the prefix is set up with, as umu-run gets it: GE-Proton's build, the
        // player's own, or none, umu-run's own.
        let set_up_with = match &proton {
            Some(home) => home.display().to_string(),
            None => own_proton(&options, &cfg.env)
                .map(|own| own.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let platform = start::linux_platform(
            UmuOptions {
                umu_run: Some(umu_run),
                prefix: Some(prefix.clone()),
                proton,
            },
            &cfg.data_dir,
            env_var(&cfg.env, "PATH").as_deref(),
            Some(cfg.data_dir.clone()),
        )
        .map_err(|e| words::couldnt_launch(&format!("{e:#}")))?;
        let log = game_log(&cfg.data_dir).map_err(|e| words::couldnt_launch(&e.to_string()))?;
        // A prefix another Proton set up is set up again now, before the ticket: that Proton
        // would upgrade it inside the game's own run, while the ticket ages (the final review, I4).
        let set_up = !start::prefix_ready(&prefix)
            || start::prefix_proton(&prefix).as_deref() != Some(set_up_with.as_str());
        self.change(|st| {
            if let GameView::Starting { first_start } = &mut st.game {
                *first_start = set_up;
            }
        });
        if set_up {
            info!("setting the Wine prefix up");
            let mut spec =
                start::prepare_spec(&platform).ok_or_else(|| words::PREFIX_FAILED.to_string())?;
            spec.env.extend(variables(&options));
            let (out, err) = log_pair(&log).map_err(|e| words::couldnt_launch(&e.to_string()))?;
            if let Err(e) =
                std::fs::create_dir_all(&spec.cwd).and_then(|()| start::mark_unfinished(&prefix))
            {
                warn!(error = %e, "couldn't ready the prefix's folder");
                return Err(words::PREFIX_FAILED.into());
            }
            let mut cmd = tokio::process::Command::from(start::command(&spec, cfg.env.clone()));
            cmd.stdin(Stdio::null()).stdout(out).stderr(err);
            match run_in_group(cmd, || self.start_cancelled(), PREFIX_LIMIT).await {
                // Stopped by a signal from elsewhere, it may not have finished: the mark stays.
                GroupEnd::Ended(status) if status.code().is_none() => {
                    warn!(%status, "umu-run createprefix was stopped");
                }
                GroupEnd::Ended(_) => {
                    if let Err(e) = start::mark_finished(&prefix)
                        .and_then(|()| start::mark_proton(&prefix, &set_up_with))
                    {
                        warn!(error = %e, "couldn't mark the prefix finished");
                    }
                }
                GroupEnd::Cancelled => return Err(words::START_CANCELLED.into()),
                GroupEnd::TooLong => return Err(words::PREFIX_TOO_LONG.into()),
                GroupEnd::NotRun(e) => warn!(error = %e, "umu-run createprefix didn't run"),
            }
            if !start::prefix_ready(&prefix) {
                return Err(words::PREFIX_FAILED.into());
            }
        }
        self.going_on()?;
        let pairs = or_cancelled(self.ticket(id), || self.start_cancelled(), LOOK_EVERY)
            .await
            .ok_or_else(|| words::START_CANCELLED.to_string())??;
        let mut spec = base_spec(&client, &platform);
        spec.env.extend(pairs);
        spec.env.extend(variables(&options));
        spec.args.extend(options.args.iter().cloned());
        let mut cmd = tokio::process::Command::from(start::command_under(
            &spec,
            &self.wrappers(&options),
            cfg.env.clone(),
        ));
        drop(spec);
        let err = log.try_clone().map_err(|e| e.to_string())?;
        cmd.stdin(Stdio::null())
            .stdout(log)
            .stderr(err)
            // Closing the launcher leaves the game running (spec L12).
            .kill_on_drop(false);
        self.going_on()?;
        let spawned = {
            // A fork while a test writes a script would make that script busy to exec.
            #[cfg(test)]
            let _spawn = crate::test_support::spawning();
            cmd.spawn()
        };
        drop(cmd);
        let mut child = spawned.map_err(|e| words::couldnt_launch(&e.to_string()))?;
        let started = Instant::now();
        let since_ms = now_ms();
        // Kept for the next launcher while the game runs, should this one close (#348).
        let mark = child.id().and_then(|pid| GameMark::of(pid, since_ms));
        if let Some(mark) = &mark
            && let Err(e) = mark.write(&cfg.data_dir)
        {
            warn!(error = %e, "couldn't note the running game for the next launcher");
        }
        self.change(|st| st.game = GameView::Running { since_ms });
        info!(server = id, "the game is running");
        let code = match child.wait().await {
            Ok(status) => i32::from(start::exit_byte(status)),
            Err(e) => {
                warn!(error = %e, "lost the game's process");
                -1
            }
        };
        info!(code, "the game ended");
        if mark.is_some() {
            GameMark::forget(&cfg.data_dir);
        }
        let notice = end_notice(code, started.elapsed());
        self.change(|st| {
            st.game = GameView::Ended { code };
            if let Some(text) = notice {
                st.notice = Some(Notice { text, error: true });
            }
        });
        Ok(())
    }

    /// umu-run: the configured one, then the settings', then `umu-run` on the launcher's `PATH`,
    /// then fetched once into the data folder (launcher spec L4).
    async fn umu_run(&self) -> Result<PathBuf, String> {
        let cfg = &self.inner.cfg;
        if let Some(path) = &cfg.umu_run {
            return Ok(path.clone());
        }
        if let Some(path) = self.inner.lock().settings.umu_run.clone() {
            return Ok(path);
        }
        let path = env_var(&cfg.env, "PATH");
        if let Some(found) = start::on_path(OsStr::new("umu-run"), path.as_deref()) {
            return Ok(found);
        }
        self.change(|st| {
            if st.client.job.is_none() {
                st.client.job = Some(JobView::Fetching {
                    tool: UMU_LAUNCHER.into(),
                    percent: None,
                });
            }
        });
        let fetched = or_cancelled(
            self.fetch_tool(tools::umu_launcher(), UMU_LAUNCHER),
            || self.start_cancelled(),
            LOOK_EVERY,
        )
        .await
        .unwrap_or_else(|| Err(words::START_CANCELLED.into()));
        self.change(|st| {
            if matches!(&st.client.job, Some(JobView::Fetching { tool, .. }) if tool == UMU_LAUNCHER) {
                st.client.job = None;
            }
        });
        fetched
    }

    /// What the game starts under, outermost first, umu-run inside them all (launcher spec
    /// Amendment B, LB2 and LB3): the launch options' wrappers, then GameMode's `gamemoderun`,
    /// where it's installed, a session bus is (the config keeps none otherwise), and its switch
    /// is on. `gamemoderun` runs what it's given in its own place, so the game's process is the
    /// one started, as without it.
    fn wrappers(&self, options: &LaunchOptions) -> Vec<std::ffi::OsString> {
        let on = self.inner.lock().settings.gamemode;
        let gamemode = self.inner.cfg.gamemoderun.iter().filter(|_| on);
        let gamemode = gamemode.map(|p| p.clone().into_os_string());
        options
            .wrappers
            .iter()
            .map(Into::into)
            .chain(gamemode)
            .collect()
    }

    /// The GE-Proton umu-run starts the game with (launcher spec Amendment B, LB1): none when the
    /// player set `PROTONPATH`, in the launch options or the launcher's environment, which asks
    /// nothing; else the latest, asked of GitHub before each start and fetched into the data
    /// folder's `proton/` when it's newer, its download shown on PLAY; else the build there; else
    /// none, umu-run's own default. Cancel start stops it at once.
    async fn proton(&self, options: &LaunchOptions) -> Option<PathBuf> {
        let cfg = &self.inner.cfg;
        // The player's own reaches umu-run as it is, umu's own names (GE-Proton, UMU-Latest, a
        // folder in compatibilitytools.d) too: never anchored to the launcher's folder (the final
        // review, I2).
        if own_proton(options, &cfg.env).is_some() {
            return None;
        }
        let dir = cfg.data_dir.join("proton");
        let Some(api) = &cfg.ge_proton else {
            return proton::installed(&dir);
        };
        let app = self.clone();
        let stop: proton::Stop = Arc::new(move || app.start_cancelled());
        let found = proton::for_play(&dir, api, &self.fetching(GE_PROTON), &stop).await;
        self.change(|st| {
            if matches!(&st.client.job, Some(JobView::Fetching { tool, .. }) if tool == GE_PROTON) {
                st.client.job = None;
            }
        });
        found
    }

    /// The login path's pairs for one game token, taken with the session this launcher holds.
    async fn ticket(&self, id: u32) -> Result<Vec<(String, String)>, String> {
        let (api, session, info, server, host) = {
            let st = self.inner.lock();
            let live = st.live.get(&id);
            let (Some(api), Some(session)) = (
                live.and_then(|l| l.api.clone()),
                live.and_then(|l| l.session.clone()),
            ) else {
                return Err(words::NO_SERVER.into());
            };
            let Some(info) = live.and_then(|l| l.info.clone()) else {
                return Err(words::api_error(
                    &ApiError::Http(String::new()),
                    &st.name(id),
                    None,
                    None,
                ));
            };
            (api, session, info, st.name(id), st.host(id))
        };
        match start::ticket_env(&api, &session, &info).await {
            Ok(pairs) => Ok(pairs),
            Err(e) => {
                let Some(answer) = e.downcast_ref::<ApiError>() else {
                    return Err(words::no_ticket(&server, &format!("{e:#}")));
                };
                if super::servers::session_over(answer) {
                    self.change(|st| {
                        if let Some(live) = st.live.get_mut(&id) {
                            (live.session, live.username, live.lobby) = (None, None, None);
                        }
                    });
                }
                Err(words::api_error(answer, &server, host.as_deref(), None))
            }
        }
    }
}

/// The folder the settings name now is the one Play's check passed (#367): one chosen between the
/// check and Starting is refused, rather than started unchecked.
fn still_checked(saved: Option<&Path>, checked: &Path) -> Result<(), IntentError> {
    if saved == Some(checked) {
        Ok(())
    } else {
        Err(refused(words::FOLDER_CHANGED))
    }
}

impl State {
    /// Why Play can't go on the active server, in words: anything but online (spec §3.12).
    fn not_playable(&self, id: u32, status: Option<&Status>) -> Option<IntentError> {
        let (server, host) = (self.name(id), self.host(id));
        let say = |e: ApiError| words::api_error(&e, &server, host.as_deref(), None);
        let text = match status {
            Some(Status::Online) => return None,
            Some(Status::Maintenance { message, until }) => say(ApiError::Maintenance {
                message: message.clone(),
                until: *until,
            }),
            Some(Status::KeyChanged) => say(ApiError::KeyChanged),
            Some(Status::OwnKey) => say(ApiError::OwnKey),
            Some(Status::Incompatible(why)) => {
                words::incompatible(&server, *why == Incompatible::Newer)
            }
            Some(Status::Offline) | None => say(ApiError::Http(String::new())),
        };
        Some(IntentError::Refused(text))
    }
}

impl App {
    /// Follows a game an earlier launcher started and left running (#348) until it ends, then lets
    /// Play go again. Its end code can't be had, as it isn't this launcher's child.
    pub(super) fn follow_left_game(&self, mark: GameMark) {
        let app = self.clone();
        self.inner.rt.spawn(async move {
            while mark.alive() {
                tokio::time::sleep(LEFT_GAME_POLL).await;
            }
            GameMark::forget(&app.inner.cfg.data_dir);
            info!("the game an earlier launcher started has ended");
            app.change(|st| {
                if st.game
                    == (GameView::Running {
                        since_ms: mark.since_ms,
                    })
                {
                    st.game = GameView::Idle;
                }
            });
        });
    }
}

/// How often a launcher looks whether a game an earlier one left running has ended.
const LEFT_GAME_POLL: Duration = Duration::from_secs(1);

/// The game a launcher started, kept in `<data dir>/game.pid` while it runs, so that the next
/// launcher knows (spec L12: one game at a time, across a restart too; #348). It names umu-run's
/// process, which lasts as long as the game: its pid, its start time in clock ticks since boot,
/// which tells it from a later process given the same pid, and when the game started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GameMark {
    pid: u32,
    start: u64,
    pub(super) since_ms: i64,
}

impl GameMark {
    const FILE: &'static str = "game.pid";

    /// The mark for process `pid`, if it's there to be marked.
    fn of(pid: u32, since_ms: i64) -> Option<GameMark> {
        Some(GameMark {
            pid,
            start: process_start(pid)?,
            since_ms,
        })
    }

    /// The mark an earlier launcher left whose game still runs, if any. A mark whose game has
    /// ended, or that doesn't read, is removed.
    pub(super) fn left_running(data_dir: &Path) -> Option<GameMark> {
        let text = std::fs::read_to_string(data_dir.join(Self::FILE)).ok()?;
        let mark = Self::parse(&text).filter(GameMark::alive);
        if mark.is_none() {
            Self::forget(data_dir);
        }
        mark
    }

    fn parse(text: &str) -> Option<GameMark> {
        let mut words = text.split_whitespace();
        let mark = GameMark {
            pid: words.next()?.parse().ok()?,
            start: words.next()?.parse().ok()?,
            since_ms: words.next()?.parse().ok()?,
        };
        words.next().is_none().then_some(mark)
    }

    /// Whether the marked process is still the same one, alive.
    fn alive(&self) -> bool {
        process_start(self.pid) == Some(self.start)
    }

    /// Writes the mark whole, through a temporary name, so that a reader never sees half of one.
    fn write(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let part = data_dir.join(format!("{}.part", Self::FILE));
        std::fs::write(
            &part,
            format!("{} {} {}\n", self.pid, self.start, self.since_ms),
        )?;
        std::fs::rename(part, data_dir.join(Self::FILE))
    }

    fn forget(data_dir: &Path) {
        let _ = std::fs::remove_file(data_dir.join(Self::FILE));
    }
}

/// When process `pid` started, in clock ticks since boot (`/proc/<pid>/stat`'s 22nd field); none
/// if there's no such process, or no `/proc` (Play is Linux-only, spec L2).
fn process_start(pid: u32) -> Option<u64> {
    proc_stat(pid).map(|(_, start)| start)
}

/// Process `pid`'s parent and its start (`/proc/<pid>/stat`'s 4th and 22nd fields).
fn proc_stat(pid: u32) -> Option<(u32, u64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command's name comes in parentheses and may hold spaces or parentheses: the fields
    // after it start at the last ')', with the state, the third field.
    let mut fields = stat.get(stat.rfind(')')? + 2..)?.split(' ');
    let parent = fields.nth(4 - 3)?.parse().ok()?;
    let start = fields.nth(22 - 5)?.parse().ok()?;
    Some((parent, start))
}

/// Whether a game is starting or running (spec L12: one at a time).
pub(super) fn playing(st: &State) -> bool {
    matches!(
        st.game,
        GameView::Starting { .. } | GameView::Running { .. }
    )
}

/// What the player is told when the game ends with `code` after `lasted`. Within [`STARTUP`] it
/// couldn't start, whatever the code, since umu-run 1.4 ends with 0 when it fails (#332); later,
/// only a code other than 0 is worth a word.
fn end_notice(code: i32, lasted: Duration) -> Option<String> {
    if lasted < STARTUP {
        Some(if code == 0 {
            words::ENDED_AT_ONCE.into()
        } else {
            words::couldnt_start(code)
        })
    } else if code != 0 {
        Some(words::ended(code))
    } else {
        None
    }
}

/// The launch options' variables for the start, `PROTONPATH` as typed among them: an empty one
/// is none, and GE-Proton plays.
fn variables(options: &LaunchOptions) -> impl Iterator<Item = (String, String)> + '_ {
    options
        .env
        .iter()
        .filter(|(name, value)| name != "PROTONPATH" || !value.is_empty())
        .cloned()
}

/// The player's own `PROTONPATH`, as umu-run gets it: the launch options', else the launcher's
/// environment's. An empty one is none, and one in the options hides the environment's, as in a
/// shell.
fn own_proton(
    options: &LaunchOptions,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Option<std::ffi::OsString> {
    let typed = options
        .env
        .iter()
        .rev()
        .find(|(name, _)| name == "PROTONPATH");
    let own = match typed {
        Some((_, value)) => Some(value.into()),
        None => env_var(env, "PROTONPATH"),
    };
    own.filter(|own: &std::ffi::OsString| !own.is_empty())
}

fn env_var(
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    name: &str,
) -> Option<std::ffi::OsString> {
    env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// The game's log, twice: for a child's stdout and its stderr.
fn log_pair(log: &std::fs::File) -> std::io::Result<(std::fs::File, std::fs::File)> {
    Ok((log.try_clone()?, log.try_clone()?))
}

/// How a command run in a process group of its own ended.
#[derive(Debug)]
enum GroupEnd {
    /// By itself: with a code, or by a signal from elsewhere.
    Ended(std::process::ExitStatus),
    Cancelled,
    TooLong,
    /// It couldn't be run, or waited for.
    NotRun(std::io::Error),
}

/// Runs `cmd` in a process group of its own until it ends, `cancelled` says so, or `limit` passes.
/// The last two stop everything it started (#348). umu-run 1.4.4 starts Proton in a session of
/// its own, and passes SIGTERM on to it. So the stop sends umu-run's group SIGTERM and gives it
/// [`GRACE`]. Then it kills every process under umu-run, and the group. umu-run is a subreaper, so
/// what it started stays under it while it lives, double forks such as wineserver included; what
/// was under it as the stop began is killed too, should umu-run end first.
async fn run_in_group(
    mut cmd: tokio::process::Command,
    cancelled: impl Fn() -> bool,
    limit: Duration,
) -> GroupEnd {
    #[cfg(unix)]
    cmd.process_group(0);
    let spawned = {
        // A fork while a test writes a script would make that script busy to exec.
        #[cfg(test)]
        let _spawn = crate::test_support::spawning();
        cmd.spawn()
    };
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return GroupEnd::NotRun(e),
    };
    let group = child.id();
    let waited = tokio::time::timeout(limit, or_cancelled(child.wait(), cancelled, LOOK_EVERY));
    let end = match waited.await {
        Ok(Some(Ok(status))) => return GroupEnd::Ended(status),
        Ok(Some(Err(e))) => return GroupEnd::NotRun(e),
        Ok(None) => GroupEnd::Cancelled,
        Err(_) => GroupEnd::TooLong,
    };
    let mut tree = group.map(descendants).unwrap_or_default();
    signal_group(group, false);
    let _ = tokio::time::timeout(GRACE, child.wait()).await;
    // What's left, once umu-run has ended or had its time.
    tree.extend(group.map(descendants).unwrap_or_default());
    kill_each(&tree);
    signal_group(group, true);
    let _ = child.start_kill();
    end
}

/// The processes under `root`, by `/proc`'s parent links, each with its start, so that a pid the
/// system has given to another process since is never killed. None off Linux, which has no
/// `/proc` (Play is Linux-only, spec L2).
fn descendants(root: u32) -> Vec<(u32, u64)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let all: Vec<(u32, u32, u64)> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| proc_stat(pid).map(|(parent, start)| (pid, parent, start)))
        .collect();
    let mut found: Vec<(u32, u64)> = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for &(pid, of, start) in &all {
            if of == parent && !found.iter().any(|&(seen, _)| seen == pid) {
                found.push((pid, start));
                parents.push(pid);
            }
        }
    }
    found
}

/// SIGKILL to each of `processes` that is still the process it was.
#[cfg(unix)]
fn kill_each(processes: &[(u32, u64)]) {
    use rustix::process::{Pid, Signal, kill_process};
    for &(pid, start) in processes {
        if process_start(pid) != Some(start) {
            continue;
        }
        if let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_each(_: &[(u32, u64)]) {}

/// SIGTERM to the process group `group` leads, or SIGKILL when `kill`. Off Unix, nothing: the
/// leader alone is killed there.
#[cfg(unix)]
fn signal_group(group: Option<u32>, kill: bool) {
    use rustix::process::{Pid, Signal, kill_process_group};
    let Some(pid) = group
        .and_then(|g| i32::try_from(g).ok())
        .and_then(Pid::from_raw)
    else {
        return;
    };
    let signal = if kill { Signal::KILL } else { Signal::TERM };
    if let Err(e) = kill_process_group(pid, signal)
        && e != rustix::io::Errno::SRCH
    {
        warn!(error = %e, "couldn't signal umu-run's process group");
    }
}

#[cfg(not(unix))]
fn signal_group(_: Option<u32>, _: bool) {}

/// `<data dir>/logs/game.log`, new for each start: what umu-run, Proton and the client print,
/// which the words "the log folder has the details" point to.
fn game_log(data_dir: &Path) -> std::io::Result<std::fs::File> {
    let logs = data_dir.join("logs");
    std::fs::create_dir_all(&logs)?;
    std::fs::File::create(logs.join("game.log"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::super::AppConfig;
    use super::*;
    use crate::manifest::Manifest;
    use crate::options::{self, LaunchOptions};
    use crate::proton::{MARKER, fakes};
    use crate::test_support::until;

    /// The config of an App on Linux whose own environment is `env`, asking `ge_proton` for
    /// GE-Proton's latest release.
    fn config_with(
        dir: &Path,
        env: Vec<(OsString, OsString)>,
        ge_proton: Option<String>,
    ) -> AppConfig {
        AppConfig {
            config_dir: dir.join("config"),
            data_dir: dir.join("data"),
            home: Some(dir.join("home")),
            os: Os::Linux,
            version: "0.5.5".into(),
            manifest: Manifest::default(),
            pinned_build: "10015727".into(),
            env,
            depot_downloader: None,
            umu_run: None,
            ge_proton,
            gamemoderun: None,
        }
    }

    /// The App of [`config_with`].
    fn app_with(dir: &Path, env: Vec<(OsString, OsString)>, ge_proton: Option<String>) -> App {
        App::start(
            config_with(dir, env, ge_proton),
            tokio::runtime::Handle::current(),
        )
    }

    #[tokio::test]
    async fn the_game_starts_under_gamemoderun_only_where_a_session_bus_is() {
        // Launcher spec Amendment B, LB2: with no session bus, gamemoderun's preload has libdbus
        // autolaunch dbus-launch, which it makes ask again, without end (Task 11's rehearsal:
        // 51,262 of them, and no game).
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config_with(dir.path(), Vec::new(), None);
        cfg.gamemoderun = Some("/usr/bin/gamemoderun".into());
        let mangohud = options::parse("mangohud %command%").unwrap();
        let app = App::start(cfg.clone(), tokio::runtime::Handle::current());
        assert_eq!(app.wrappers(&mangohud), [OsString::from("mangohud")]);
        drop(app);
        let bus = ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus");
        cfg.env.push((bus.0.into(), bus.1.into()));
        let app = App::start(cfg, tokio::runtime::Handle::current());
        assert_eq!(
            app.wrappers(&mangohud),
            ["mangohud", "/usr/bin/gamemoderun"].map(OsString::from)
        );
    }

    /// An address nothing answers on: the connection is refused at once.
    const NOWHERE: &str = "http://127.0.0.1:1/api";

    #[tokio::test]
    async fn a_players_own_protonpath_asks_github_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fakes::github("GE-Proton11-9", fakes::ge("GE-Proton11-9"), None).await;
        let own = vec![("PROTONPATH".into(), "/home/me/proton".into())];
        let app = app_with(dir.path(), own, Some(fake.api.clone()));
        // No GE-Proton: the environment's own reaches umu-run as it is (final review I2).
        assert_eq!(app.proton(&LaunchOptions::default()).await, None);
        assert_eq!((fake.asked(), fake.hits()), (0, 0));
        // An empty one is none: the latest GE-Proton plays.
        let other = tempfile::tempdir().unwrap();
        let empty = vec![("PROTONPATH".into(), "".into())];
        let app = app_with(other.path(), empty, Some(fake.api.clone()));
        let home = other.path().join("data/proton/GE-Proton11-9");
        assert_eq!(app.proton(&LaunchOptions::default()).await, Some(home));
        assert_eq!((fake.asked(), fake.hits()), (1, 1));
    }

    #[tokio::test]
    async fn a_launch_options_protonpath_takes_ge_protons_place_and_the_environments() {
        // Launcher spec Amendment B, LB3: the player's own Proton, set where Settings shows it.
        let dir = tempfile::tempdir().unwrap();
        let fake = fakes::github("GE-Proton11-9", fakes::ge("GE-Proton11-9"), None).await;
        let env = vec![("PROTONPATH".into(), "/from/the/environment".into())];
        let app = app_with(dir.path(), env, Some(fake.api.clone()));
        let own = options::parse("PROTONPATH=GE-Proton %command%").unwrap();
        // No GE-Proton: the options' own reaches umu-run as typed (the final review, I2), before
        // the environment's.
        assert_eq!(app.proton(&own).await, None);
        assert_eq!((fake.asked(), fake.hits()), (0, 0));
        let env = &app.inner.cfg.env;
        assert_eq!(own_proton(&own, env), Some("GE-Proton".into()));
        let mut vars: Vec<(String, String)> = variables(&own).collect();
        vars.retain(|(name, _)| name == "PROTONPATH");
        assert_eq!(vars, [("PROTONPATH".to_string(), "GE-Proton".to_string())]);
        // An empty one in the options hides the environment's, as in a shell, and GE-Proton plays.
        let empty = options::parse("PROTONPATH= %command%").unwrap();
        assert_eq!(own_proton(&empty, env), None);
        assert_eq!(variables(&empty).count(), 0);
    }

    #[tokio::test]
    async fn play_gets_the_latest_ge_proton_into_its_data_folder_or_plays_the_one_it_has() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fakes::github("GE-Proton11-9", fakes::ge("GE-Proton11-9"), None).await;
        let app = app_with(dir.path(), Vec::new(), Some(fake.api.clone()));
        let home = dir.path().join("data/proton/GE-Proton11-9");
        assert_eq!(
            app.proton(&LaunchOptions::default()).await,
            Some(home.clone())
        );
        assert!(home.join(MARKER).is_file());
        assert_eq!((fake.asked(), fake.hits()), (1, 1));
        assert!(app.snapshot().job.is_none(), "the fetch's job ends with it");
        // Offline, the build it has plays; with none yet, umu-run's own default.
        let offline = app_with(dir.path(), Vec::new(), Some(NOWHERE.into()));
        assert_eq!(offline.proton(&LaunchOptions::default()).await, Some(home));
        let empty = tempfile::tempdir().unwrap();
        let none = app_with(empty.path(), Vec::new(), Some(NOWHERE.into()));
        assert_eq!(none.proton(&LaunchOptions::default()).await, None);
    }

    #[tokio::test]
    async fn ge_protons_download_shows_on_play_and_cancel_start_stops_it_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let protons = dir.path().join("data/proton");
        let have = fakes::built(&protons, "GE-Proton10-34");
        let zeros = format!("{}  GE-Proton11-9-x86_64.tar.gz", "0".repeat(128));
        let stalled = Some(fakes::stalled().await);
        let fake = fakes::github_at("GE-Proton11-9", Vec::new(), Some(zeros), stalled).await;
        let app = app_with(dir.path(), Vec::new(), Some(fake.api.clone()));
        app.change(|st| st.game = GameView::Starting { first_start: false });
        let start = tokio::spawn({
            let app = app.clone();
            async move { app.proton(&LaunchOptions::default()).await }
        });
        // Its first kilobyte of a megabyte came, and then nothing.
        until(&app, |s| {
            matches!(&s.job, Some(JobView::Fetching { tool, percent: Some(_) }) if tool == GE_PROTON)
        })
        .await;
        app.cancel_start();
        let found = tokio::time::timeout(Duration::from_secs(5), start)
            .await
            .expect("Cancel start stops it at once")
            .unwrap();
        assert_eq!(found, Some(have));
        assert!(app.snapshot().job.is_none());
        let left: Vec<_> = std::fs::read_dir(&protons)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["GE-Proton10-34"], "nothing half-made is left");
    }

    #[test]
    fn a_game_that_ends_at_once_couldnt_start_whatever_its_code() {
        // umu-run 1.4.4 logs its failure and ends with 0 (#332; rehearsal r6 for #302).
        let soon = Duration::from_millis(190);
        assert_eq!(end_notice(0, soon).as_deref(), Some(words::ENDED_AT_ONCE));
        assert_eq!(end_notice(3, soon), Some(words::couldnt_start(3)));
    }

    #[test]
    fn a_mark_reads_three_numbers_and_nothing_else() {
        let mark = GameMark::parse("4242 987654 1792128679626\n").unwrap();
        assert_eq!(
            (mark.pid, mark.start, mark.since_ms),
            (4242, 987654, 1792128679626)
        );
        for bad in [
            "",
            "4242 987654",
            "4242 987654 1 2",
            "x 987654 1",
            "4242 -1 1",
        ] {
            assert_eq!(GameMark::parse(bad), None, "{bad:?}");
        }
    }

    /// Linux only: the mark reads `/proc`, as Play runs only there (spec L2).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_mark_whose_game_has_ended_is_forgotten_and_a_live_one_is_kept() {
        // #348: a pid the system gave to another process since doesn't hold Play.
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let start = process_start(me).expect("this process has a start time");
        let stale = GameMark {
            pid: me,
            start: start + 1,
            since_ms: 1,
        };
        stale.write(dir.path()).unwrap();
        assert_eq!(GameMark::left_running(dir.path()), None);
        assert!(
            !dir.path().join(GameMark::FILE).exists(),
            "a stale mark stays"
        );
        let live = GameMark::of(me, 2).unwrap();
        live.write(dir.path()).unwrap();
        assert_eq!(GameMark::left_running(dir.path()), Some(live));
        assert_eq!(process_start(u32::MAX), None);
    }

    #[test]
    fn play_starts_only_the_folder_its_check_passed() {
        // #367: a folder chosen between Play's quick check and Starting would have been launched
        // unchecked.
        let checked = Path::new("/games/Swords of Legends Online");
        assert!(still_checked(Some(checked), checked).is_ok());
        for now in [Some(Path::new("/games/another copy")), None] {
            assert_eq!(
                still_checked(now, checked).unwrap_err().to_string(),
                words::FOLDER_CHANGED,
                "{now:?}"
            );
        }
    }

    #[test]
    fn later_only_a_code_other_than_0_is_worth_a_word() {
        let later = STARTUP + Duration::from_secs(1);
        assert_eq!(end_notice(0, later), None);
        assert_eq!(end_notice(3, later), Some(words::ended(3)));
    }

    #[test]
    fn the_words_for_a_prefix_past_its_limit_name_the_limit() {
        let minutes = format!("over {} minutes", PREFIX_LIMIT.as_secs() / 60);
        assert!(words::PREFIX_TOO_LONG.contains(&minutes));
    }

    /// #348: past its limit, a command is stopped with everything it started, even what it put in
    /// a session of its own that ignores SIGTERM, as umu-run 1.4.4 does with Proton.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_run_past_its_limit_ends_with_everything_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        let umu_run = dir.path().join("umu-run");
        std::fs::write(
            &umu_run,
            format!(
                r#"setsid sh -c 'trap "" TERM; sleep 600 & echo $! > "$0"; wait' '{}' &
proton=$!
trap 'kill -TERM "$proton" 2>/dev/null' TERM
while kill -0 "$proton" 2>/dev/null; do wait "$proton"; done
"#,
                child.display()
            ),
        )
        .unwrap();
        let mut cmd = tokio::process::Command::new("sh");
        // Nothing left behind may hold the test's own output open.
        cmd.arg(&umu_run)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let started = Instant::now();
        let end = run_in_group(cmd, || false, Duration::from_secs(1)).await;
        assert!(matches!(end, GroupEnd::TooLong), "{end:?}");
        assert!(started.elapsed() < GRACE + Duration::from_secs(5));
        let pid: u32 = std::fs::read_to_string(&child)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let gone = Instant::now();
        while alive(pid) {
            assert!(
                gone.elapsed() < Duration::from_secs(10),
                "its child outlived it"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Whether `pid` runs: a process that has ended but isn't reaped yet doesn't.
    #[cfg(target_os = "linux")]
    fn alive(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .is_some_and(|(_, rest)| !rest.trim_start().starts_with(['Z', 'X']))
        })
    }
}
