//! Play (launcher spec §3.7; L2, L12; plan LP8): the client started through umu-run with a fresh
//! ticket in its environment, and followed until it ends. The ticket's pairs go to the client's
//! environment only. The launcher stays open while the game runs, and closing it leaves the game
//! running.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::time::Instant;
use tracing::{info, warn};

use super::{
    App, CheckView, GameView, IntentError, JobView, LevelView, Notice, State, now_ms, refused,
    words,
};
use crate::api::ApiError;
use crate::launch::base_spec;
use crate::probe::{self, Incompatible, Status};
use crate::settings::Os;
use crate::start::{self, UmuOptions};
use crate::tools;

/// An end this soon after the start means the game couldn't start, whatever its code (spec §3.12;
/// #332).
const STARTUP: Duration = Duration::from_secs(15);
const UMU_LAUNCHER: &str = "umu-launcher";

impl App {
    /// Play on the active server: the refusals, then a sign-in if there's no session (LP8), the
    /// quick check (L11), and the start, which carries on in its own task.
    pub(super) async fn play(&self) -> Result<(), IntentError> {
        if self.inner.cfg.os != Os::Linux {
            return Err(refused(words::NOT_HERE));
        }
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
        match self.check_here(LevelView::Quick).await? {
            Some(CheckView::Verified { .. }) => {}
            Some(CheckView::NeedsRepair { .. }) => return Err(refused(words::NEEDS_REPAIR)),
            Some(_) => return Err(refused(words::NO_FOLDER)),
            // Stopped, or it failed: its notice says so.
            None => return Ok(()),
        }
        let prefix = self.inner.cfg.data_dir.join("prefix");
        self.change(|st| {
            if playing(st) {
                return Err(refused(words::RUNNING));
            }
            st.game = GameView::Starting {
                first_start: !start::prefix_ready(&prefix),
            };
            Ok(())
        })?;
        let app = self.clone();
        self.inner.rt.spawn(async move {
            if let Err(text) = app.start_game(id, prefix).await {
                app.change(|st| {
                    st.game = GameView::Idle;
                    st.notice = Some(Notice { text, error: true });
                });
            }
        });
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
    async fn start_game(&self, id: u32, prefix: PathBuf) -> Result<(), String> {
        let cfg = &self.inner.cfg;
        let umu_run = self.umu_run().await?;
        let var = |name: &str| env_var(&cfg.env, name);
        let platform = start::linux_platform(
            UmuOptions {
                umu_run: Some(umu_run),
                prefix: Some(prefix.clone()),
                proton: var("PROTONPATH").map(PathBuf::from),
            },
            &cfg.data_dir,
            var("PATH").as_deref(),
            Some(cfg.data_dir.clone()),
        )
        .map_err(|e| words::couldnt_launch(&format!("{e:#}")))?;
        let log = game_log(&cfg.data_dir).map_err(|e| words::couldnt_launch(&e.to_string()))?;
        if !start::prefix_ready(&prefix) {
            info!("making the Wine prefix");
            let spec =
                start::prepare_spec(&platform).ok_or_else(|| words::PREFIX_FAILED.to_string())?;
            let (env, out) = (cfg.env.clone(), log.try_clone().map_err(|e| e.to_string())?);
            let made = tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(&spec.cwd)?;
                let err = out.try_clone()?;
                start::command(&spec, env)
                    .stdin(Stdio::null())
                    .stdout(out)
                    .stderr(err)
                    .status()
            })
            .await;
            if let Ok(Err(e)) | Err(e) = made.map_err(std::io::Error::other) {
                warn!(error = %e, "umu-run createprefix didn't run");
            }
            if !start::prefix_ready(&prefix) {
                return Err(words::PREFIX_FAILED.into());
            }
        }
        let pairs = self.ticket(id).await?;
        let client = self
            .inner
            .lock()
            .settings
            .client
            .clone()
            .ok_or_else(|| words::NO_FOLDER.to_string())?;
        let mut spec = base_spec(&client, &platform);
        spec.env.extend(pairs);
        let mut cmd = tokio::process::Command::from(start::command(&spec, cfg.env.clone()));
        drop(spec);
        let err = log.try_clone().map_err(|e| e.to_string())?;
        cmd.stdin(Stdio::null())
            .stdout(log)
            .stderr(err)
            // Closing the launcher leaves the game running (spec L12).
            .kill_on_drop(false);
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
        let fetched = self.fetch_tool(tools::umu_launcher(), UMU_LAUNCHER).await;
        self.change(|st| {
            if matches!(&st.client.job, Some(JobView::Fetching { tool, .. }) if tool == UMU_LAUNCHER) {
                st.client.job = None;
            }
        });
        fetched
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
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command's name comes in parentheses and may hold spaces or parentheses: the fields
    // after it start at the last ')', with the state, the third field.
    let after = stat.get(stat.rfind(')')? + 2..)?;
    after.split(' ').nth(22 - 3)?.parse().ok()
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

fn env_var(
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    name: &str,
) -> Option<std::ffi::OsString> {
    env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// `<data dir>/logs/game.log`, new for each start: what umu-run, Proton and the client print,
/// which the words "the log folder has the details" point to.
fn game_log(data_dir: &Path) -> std::io::Result<std::fs::File> {
    let logs = data_dir.join("logs");
    std::fs::create_dir_all(&logs)?;
    std::fs::File::create(logs.join("game.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn later_only_a_code_other_than_0_is_worth_a_word() {
        let later = STARTUP + Duration::from_secs(1);
        assert_eq!(end_notice(0, later), None);
        assert_eq!(end_notice(3, later), Some(words::ended(3)));
    }
}
