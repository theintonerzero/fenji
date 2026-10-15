//! The player's copy (launcher spec §3.5, §3.6; L2, L3, L11; plan LP6): its checks, and
//! DepotDownloader's downloads and repairs in a terminal window of their own, where the player
//! signs in to Steam (never in Fenji). One job runs at a time. A check runs on a blocking thread;
//! a download is followed through the files its run leaves in its folder.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use super::{
    App, CheckView, FileProblem, IntentError, JobView, LevelView, Notice, State, now_ms, refused,
    words,
};
use crate::depot::{self, DepotRun, RunState, Terminal};
use crate::manifest::Manifest;
use crate::settings::Os;
use crate::tools;
use crate::verify::{self, Level, Problem, Progress, Report, Stopped};

/// How often a running check or fetch publishes its progress, at most.
const PUBLISH_EVERY: Duration = Duration::from_millis(100);
/// How often a download's run folder is read.
const POLL_EVERY: Duration = Duration::from_millis(500);
const DEPOT_DOWNLOADER: &str = "DepotDownloader";

/// The player's copy as this run knows it.
pub(super) struct Client {
    pub(super) check: CheckView,
    pub(super) job: Option<JobView>,
    /// The running check's stop flag.
    stop: Option<Arc<AtomicBool>>,
    /// The running download, once its terminal has opened.
    run: Option<DepotRun>,
    /// Whether the player cancelled the running download.
    cancelled: bool,
}

impl Default for Client {
    fn default() -> Self {
        Client {
            check: CheckView::None,
            job: None,
            stop: None,
            run: None,
            cancelled: false,
        }
    }
}

/// A check about to run: where, against which files, how deep, and its stop flag.
pub(super) struct CheckJob {
    folder: PathBuf,
    manifest: Manifest,
    level: LevelView,
    stop: Arc<AtomicBool>,
}

impl From<LevelView> for Level {
    fn from(level: LevelView) -> Level {
        match level {
            LevelView::Quick => Level::Quick,
            LevelView::Full => Level::Full,
        }
    }
}

/// Lets progress through at most every [`PUBLISH_EVERY`]: it comes far faster than anyone reads.
struct Throttle(Mutex<Option<Instant>>);

impl Throttle {
    fn new() -> Self {
        Throttle(Mutex::new(None))
    }

    fn due(&self) -> bool {
        let now = Instant::now();
        let mut last = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_some_and(|at| now - at < PUBLISH_EVERY) {
            return false;
        }
        *last = Some(now);
        true
    }
}

/// One job at a time.
fn free(st: &State) -> Result<(), IntentError> {
    match st.client.job {
        Some(_) => Err(refused(words::BUSY)),
        None => Ok(()),
    }
}

fn waiting_for_steam(repair: bool) -> JobView {
    JobView::Downloading {
        repair,
        percent: None,
        file: None,
        waiting_for_steam: true,
    }
}

impl App {
    /// ChooseFolder: saves the folder, then checks it quickly (spec L11).
    pub(super) fn choose_folder(&self, path: String) -> Result<(), IntentError> {
        if path.trim().is_empty() {
            return Err(refused(words::NO_FOLDER));
        }
        let job = self.change(|st| {
            free(st)?;
            st.settings.client = Some(PathBuf::from(&path));
            self.save(st);
            self.begin_check(st, LevelView::Quick, None)
        })?;
        self.run_check(job);
        Ok(())
    }

    pub(super) fn check(&self, level: LevelView) -> Result<(), IntentError> {
        let job = self.change(|st| self.begin_check(st, level, None))?;
        self.run_check(job);
        Ok(())
    }

    /// The saved folder's quick check when the launcher starts (spec L11).
    pub(super) fn check_at_start(&self) {
        let job = self.change(|st| {
            st.settings.client.as_ref()?;
            self.begin_check(st, LevelView::Quick, None).ok()
        });
        if let Some(job) = job {
            self.run_check(job);
        }
    }

    pub(super) fn stop_check(&self) {
        self.change(|st| {
            if let Some(stop) = &st.client.stop {
                stop.store(true, Ordering::Relaxed);
            }
        });
    }

    /// Marks a check of the saved folder as the running job: of `only` those paths, or of every
    /// file.
    fn begin_check(
        &self,
        st: &mut State,
        level: LevelView,
        only: Option<&[String]>,
    ) -> Result<CheckJob, IntentError> {
        free(st)?;
        let folder = st
            .settings
            .client
            .clone()
            .ok_or_else(|| refused(words::NO_FOLDER))?;
        let all = &self.inner.cfg.manifest;
        let manifest = match only {
            None => all.clone(),
            Some(paths) => Manifest {
                entries: all
                    .entries
                    .iter()
                    .filter(|e| paths.contains(&e.path))
                    .cloned()
                    .collect(),
            },
        };
        let stop = Arc::new(AtomicBool::new(false));
        st.client.stop = Some(stop.clone());
        st.client.job = Some(JobView::Checking {
            level,
            files_done: 0,
            files_total: manifest.entries.len(),
            percent: 0.0,
        });
        Ok(CheckJob {
            folder,
            manifest,
            level,
            stop,
        })
    }

    /// Runs the check in its own task.
    fn run_check(&self, job: CheckJob) {
        let app = self.clone();
        self.inner.rt.spawn(async move {
            app.run_check_here(job).await;
        });
    }

    /// Checks the saved folder now, as a job, and gives back what the check found: `None` when
    /// it was stopped or failed (its notice says which).
    pub(super) async fn check_here(
        &self,
        level: LevelView,
    ) -> Result<Option<CheckView>, IntentError> {
        let job = self.change(|st| self.begin_check(st, level, None))?;
        Ok(self.run_check_here(job).await)
    }

    /// Runs the check on a blocking thread, publishing its progress, then its result.
    async fn run_check_here(&self, job: CheckJob) -> Option<CheckView> {
        let CheckJob {
            folder,
            manifest,
            level,
            stop,
        } = job;
        let (reporter, looked) = (self.clone(), folder.clone());
        let report = tokio::task::spawn_blocking(move || {
            let throttle = Throttle::new();
            let progress = |p: Progress| {
                if throttle.due() {
                    reporter.checking(level, p);
                }
            };
            verify::verify_with(&looked, &manifest, level.into(), &progress, &stop)
        })
        .await;
        let report = report.map_err(anyhow::Error::from).and_then(|r| r);
        self.checked(level, &folder, report)
    }

    fn checking(&self, level: LevelView, p: Progress) {
        self.change(|st| {
            if matches!(st.client.job, Some(JobView::Checking { .. })) {
                st.client.job = Some(JobView::Checking {
                    level,
                    files_done: p.files_done,
                    files_total: p.files_total,
                    percent: p.percent(),
                });
            }
        });
    }

    /// A check's end, and what it found (`None` when it was stopped or failed). Every file the
    /// manifest lists counts toward `fine`, so a repair's check of just its files still reads as
    /// the whole copy.
    fn checked(
        &self,
        level: LevelView,
        folder: &Path,
        result: anyhow::Result<Report>,
    ) -> Option<CheckView> {
        let at_ms = now_ms();
        let total = self.inner.cfg.manifest.entries.len();
        self.change(|st| {
            (st.client.job, st.client.stop) = (None, None);
            match result {
                Ok(report) => {
                    let failing = report.failing();
                    info!(
                        files = total,
                        failing = failing.len(),
                        "checked the game's files"
                    );
                    st.client.check = if failing.is_empty() {
                        CheckView::Verified { level, at_ms }
                    } else {
                        CheckView::NeedsRepair {
                            fine: total.saturating_sub(failing.len()),
                            files: failing
                                .iter()
                                .map(|path| FileProblem {
                                    path: path.clone(),
                                    problem: report.problem(path).unwrap_or(Problem::Missing),
                                })
                                .collect(),
                            at_ms,
                        }
                    };
                }
                Err(e) if e.is::<Stopped>() => {
                    st.notice = Some(Notice {
                        text: words::CHECK_STOPPED.into(),
                        error: false,
                    });
                    return None;
                }
                Err(_) if !folder.is_dir() => st.client.check = CheckView::FolderMissing,
                Err(e) => {
                    warn!(error = %format!("{e:#}"), "the check failed");
                    st.notice = Some(Notice {
                        text: words::check_failed(&format!("{e:#}")),
                        error: true,
                    });
                    return None;
                }
            }
            Some(st.client.check.clone())
        })
    }

    /// Download: DepotDownloader fetches the pinned build into `folder`, then every file is
    /// checked.
    pub(super) fn download(&self, folder: String) -> Result<(), IntentError> {
        if folder.trim().is_empty() {
            return Err(refused(words::NO_FOLDER));
        }
        self.start_download(PathBuf::from(folder), None)
    }

    /// Repair: fetches again only the files the last check found wrong (plan LP6).
    pub(super) fn repair(&self) -> Result<(), IntentError> {
        let (folder, files) = {
            let st = self.inner.lock();
            let CheckView::NeedsRepair { files, .. } = &st.client.check else {
                return Err(refused(words::NOTHING_TO_REPAIR));
            };
            let folder = st
                .settings
                .client
                .clone()
                .ok_or_else(|| refused(words::NO_FOLDER))?;
            (folder, files.iter().map(|f| f.path.clone()).collect())
        };
        self.start_download(folder, Some(files))
    }

    fn start_download(
        &self,
        folder: PathBuf,
        files: Option<Vec<String>>,
    ) -> Result<(), IntentError> {
        let cfg = &self.inner.cfg;
        if cfg.os != Os::Linux {
            return Err(refused(words::DOWNLOAD_LINUX_ONLY));
        }
        let terminal = self.terminal().ok_or_else(|| refused(words::NO_TERMINAL))?;
        let repair = files.is_some();
        self.change(|st| {
            free(st)?;
            st.client.cancelled = false;
            st.client.job = Some(match cfg.depot_downloader {
                Some(_) => waiting_for_steam(repair),
                None => JobView::Fetching {
                    tool: DEPOT_DOWNLOADER.into(),
                    percent: None,
                },
            });
            Ok(())
        })?;
        info!(folder = %folder.display(), repair, "a download starts");
        let app = self.clone();
        self.inner
            .rt
            .spawn(async move { app.downloading(terminal, folder, files).await });
        Ok(())
    }

    /// The terminal DepotDownloader opens in: the configured one, or else the first there is.
    fn terminal(&self) -> Option<Terminal> {
        let cfg = &self.inner.cfg;
        if let Some(t) = &cfg.terminal {
            return Some(t.clone());
        }
        let var = |name: &str| {
            cfg.env
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        let path = var("PATH");
        depot::find_terminal(&var, &|name| {
            crate::start::on_path(OsStr::new(name), path.as_deref())
        })
    }

    async fn downloading(&self, terminal: Terminal, folder: PathBuf, files: Option<Vec<String>>) {
        let repair = files.is_some();
        let dd = match self.depot_downloader().await {
            Ok(dd) => dd,
            Err(text) => return self.download_ended(text, true),
        };
        if self.inner.lock().client.cancelled {
            return self.download_ended(words::DOWNLOAD_CANCELLED.into(), false);
        }
        let job = depot::Job {
            dd,
            folder: folder.clone(),
            files: files.clone(),
        };
        let started = {
            // A fork while a test writes a script would make that script busy to exec.
            #[cfg(test)]
            let _spawn = crate::test_support::spawning();
            DepotRun::start(&terminal, &job, &self.run_dir())
        };
        let run = match started {
            Ok(run) => run,
            Err(e) => {
                warn!(error = %e, "the terminal for DepotDownloader didn't open");
                return self.download_ended(words::NO_TERMINAL.into(), true);
            }
        };
        self.change(|st| {
            st.client.run = Some(run.clone());
            st.client.job = Some(waiting_for_steam(repair));
        });
        let mut tick = tokio::time::interval(POLL_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let cancelled = self.inner.lock().client.cancelled;
            if cancelled {
                // Again on each look: its process group may not have existed at the first.
                let _ = run.cancel();
            }
            let (text, error) = match run.state() {
                RunState::Waiting => {
                    self.change(|st| st.client.job = Some(waiting_for_steam(repair)));
                    continue;
                }
                RunState::Running { percent, file } => {
                    self.change(|st| {
                        st.client.job = Some(JobView::Downloading {
                            repair,
                            percent: Some(percent),
                            file: Some(file),
                            waiting_for_steam: false,
                        });
                    });
                    continue;
                }
                RunState::Ended { code: 0 } => return self.downloaded(folder, files),
                _ if cancelled => (words::DOWNLOAD_CANCELLED.to_string(), false),
                RunState::Ended { code } => (words::download_stopped(code), true),
                RunState::Lost => (words::WINDOW_CLOSED.to_string(), true),
            };
            return self.download_ended(text, error);
        }
    }

    /// DepotDownloader's program: the configured one, or else fetched once into the data folder.
    async fn depot_downloader(&self) -> Result<PathBuf, String> {
        match &self.inner.cfg.depot_downloader {
            Some(dd) => Ok(dd.clone()),
            None => {
                self.fetch_tool(tools::depot_downloader(), DEPOT_DOWNLOADER)
                    .await
            }
        }
    }

    /// A pinned tool, fetched once into the data folder (launcher spec L4), while the job shows
    /// `Fetching` with its progress.
    pub(super) async fn fetch_tool(
        &self,
        tool: tools::Tool,
        name: &'static str,
    ) -> Result<PathBuf, String> {
        let throttle = Throttle::new();
        let progress = |done: u64, total: Option<u64>| {
            if !throttle.due() {
                return;
            }
            let percent = total
                .filter(|&t| t > 0)
                .map(|t| (done as f64 * 100.0 / t as f64) as f32);
            self.change(|st| {
                if matches!(&st.client.job, Some(JobView::Fetching { tool, .. }) if tool == name) {
                    st.client.job = Some(JobView::Fetching {
                        tool: name.into(),
                        percent,
                    });
                }
            });
        };
        tools::ensure(&tool, &self.inner.cfg.data_dir.join("tools"), &progress)
            .await
            .map_err(|e| words::tool_failed(name, &e.to_string()))
    }

    /// A fresh run folder: `<data dir>/depot/<unix ms>`.
    fn run_dir(&self) -> PathBuf {
        let base = self.inner.cfg.data_dir.join("depot");
        let mut ms = now_ms();
        loop {
            let dir = base.join(ms.to_string());
            if !dir.exists() {
                return dir;
            }
            ms += 1;
        }
    }

    /// DepotDownloader finished: a download's folder becomes the client folder, and every file of
    /// it is checked; a repair checks the files it fetched.
    fn downloaded(&self, folder: PathBuf, files: Option<Vec<String>>) {
        info!("DepotDownloader finished");
        let job = self.change(|st| {
            (st.client.job, st.client.run) = (None, None);
            if files.is_none() {
                st.settings.client = Some(folder);
                self.save(st);
            }
            self.begin_check(st, LevelView::Full, files.as_deref())
        });
        match job {
            Ok(job) => self.run_check(job),
            Err(e) => warn!(error = %e, "no check after the download"),
        }
    }

    fn download_ended(&self, text: String, error: bool) {
        info!(%text, "the download ended");
        self.change(|st| {
            (st.client.job, st.client.run, st.client.cancelled) = (None, None, false);
            st.notice = Some(Notice { text, error });
        });
    }

    pub(super) fn cancel_download(&self) {
        let run = self.change(|st| {
            if !matches!(
                st.client.job,
                Some(JobView::Fetching { .. } | JobView::Downloading { .. })
            ) {
                return None;
            }
            st.client.cancelled = true;
            st.client.run.clone()
        });
        if let Some(run) = run {
            let _ = run.cancel();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::{App, AppConfig, CheckView, Intent, JobView, LevelView, words};
    use crate::depot::Terminal;
    use crate::manifest::Manifest;
    use crate::settings::Os;
    use crate::test_support::{spawning, until};
    use crate::verify::Problem;

    /// What the fake DepotDownloader does.
    enum FakeDd {
        /// Writes the build's files (or just its file list's) into `-dir`, with progress lines,
        /// and ends with `code`.
        WritesFiles {
            code: i32,
        },
        Exits {
            code: i32,
        },
        Hangs,
    }

    /// A small build in a temp folder: its files in `pristine` (the fake DepotDownloader's
    /// source) and again in `good` (a player's copy), and their manifest.
    struct Testbed {
        _dir: tempfile::TempDir,
        root: PathBuf,
        good: PathBuf,
        pristine: PathBuf,
        manifest: Manifest,
        fakes: AtomicUsize,
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    impl Testbed {
        fn new(files: &[(&str, usize)]) -> Testbed {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            let (good, pristine) = (root.join("good"), root.join("pristine"));
            for (path, size) in files {
                let bytes: Vec<u8> = (0..*size).map(|i| (i % 251) as u8).collect();
                for base in [&good, &pristine] {
                    let p = base.join(path);
                    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                    std::fs::write(p, &bytes).unwrap();
                }
            }
            let manifest = Manifest::generate(&pristine).unwrap();
            Testbed {
                _dir: dir,
                root,
                good,
                pristine,
                manifest,
                fakes: AtomicUsize::new(0),
            }
        }

        fn config(&self, os: Os) -> AppConfig {
            AppConfig {
                config_dir: self.root.join("config"),
                data_dir: self.root.join("data"),
                home: Some(self.root.join("home")),
                os,
                version: "0.5.0".into(),
                manifest: self.manifest.clone(),
                pinned_build: "10015727".into(),
                env: vec![("PATH".into(), std::env::var_os("PATH").unwrap_or_default())],
                depot_downloader: None,
                terminal: None,
                umu_run: None,
            }
        }

        fn app(&self) -> App {
            self.app_on(Os::Linux)
        }

        fn app_on(&self, os: Os) -> App {
            App::start(self.config(os), tokio::runtime::Handle::current())
        }

        /// An app whose DepotDownloader and terminal are fakes: the terminal starts its command
        /// in a new session and returns at once, as gnome-terminal does.
        fn app_with_fakes(&self, dd: FakeDd) -> App {
            let n = self.fakes.fetch_add(1, Ordering::Relaxed);
            let body = match dd {
                FakeDd::WritesFiles { code } => {
                    let all: Vec<&str> = self
                        .manifest
                        .entries
                        .iter()
                        .map(|e| e.path.as_str())
                        .collect();
                    format!(
                        r#"dir= list=
while [ $# -gt 0 ]; do
  case $1 in -dir) dir=$2; shift ;; -filelist) list=$2; shift ;; esac
  shift
done
echo 'Scan this QR code with the Steam app'
if [ -n "$list" ]; then cp "$list" '{root}/filelist'; set -- $(cat "$list"); else set -- {all}; fi
for f in "$@"; do
  mkdir -p "$dir/$(dirname "$f")" && cp '{pristine}'/"$f" "$dir/$f" && echo " 50.00% $f"
done
exit {code}"#,
                        root = self.root.display(),
                        pristine = self.pristine.display(),
                        all = all.join(" "),
                    )
                }
                FakeDd::Exits { code } => format!("echo 'Scan this QR code'; exit {code}"),
                FakeDd::Hangs => "echo 'Scan this QR code'; exec sleep 600".into(),
            };
            let mut cfg = self.config(Os::Linux);
            {
                let _spawn = spawning();
                cfg.depot_downloader = Some(script(&self.root, &format!("dd-{n}"), &body));
                cfg.terminal = Some(Terminal {
                    program: script(
                        &self.root,
                        &format!("term-{n}"),
                        r#"setsid "$@" </dev/null >/dev/null 2>&1 &"#,
                    ),
                    before: vec![],
                });
            }
            App::start(cfg, tokio::runtime::Handle::current())
        }

        /// The paths the fake DepotDownloader was given in a repair's file list.
        fn fake_dd_filelist(&self) -> Vec<String> {
            std::fs::read_to_string(self.root.join("filelist"))
                .unwrap()
                .lines()
                .map(String::from)
                .collect()
        }

        /// Closes the running download's terminal window: its session ends, the script and
        /// DepotDownloader with it. Under `setsid`, the script's process group (its `pgid` file)
        /// is the terminal's session.
        async fn kill_fake_terminal_session(&self) {
            let runs = self.root.join("data/depot");
            let pgid = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    // A run that hasn't ended, and whose script is alive.
                    let alive = std::fs::read_dir(&runs)
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .filter(|run| !run.join("status").exists())
                        .filter_map(|run| std::fs::read_to_string(run.join("pgid")).ok())
                        .map(|id| id.trim().to_string())
                        .find(|id| signal(id, "-0"));
                    if let Some(id) = alive {
                        return id;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("the fake terminal never ran its script");
            assert!(signal(&pgid, "-TERM"));
        }
    }

    /// `kill <signal> -- -<pgid>`: whether the process group took it.
    fn signal(pgid: &str, signal: &str) -> bool {
        let _spawn = spawning();
        std::process::Command::new("kill")
            .args([signal, "--", &format!("-{pgid}")])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn checked(level: LevelView) -> impl Fn(&super::super::Snapshot) -> bool {
        move |s| matches!(s.client.check, CheckView::Verified { level: l, .. } if l == level)
    }

    #[tokio::test]
    async fn choosing_a_folder_checks_it_and_names_each_bad_file() {
        let t = Testbed::new(&[("a.bin", 1000), ("bin64/b.dll", 10)]);
        let app = t.app();
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        let s = until(&app, checked(LevelView::Quick)).await;
        assert_eq!(s.client.folder.as_deref(), Some(t.good.to_str().unwrap()));
        std::fs::write(t.good.join("a.bin"), b"short").unwrap();
        app.intent(Intent::Check {
            level: LevelView::Full,
        })
        .await
        .unwrap();
        let s = until(&app, |s| {
            matches!(s.client.check, CheckView::NeedsRepair { .. })
        })
        .await;
        let CheckView::NeedsRepair { fine, files, .. } = s.client.check else {
            unreachable!()
        };
        assert_eq!(
            (fine, files.len(), files[0].path.as_str(), files[0].problem),
            (1, 1, "a.bin", Problem::WrongSize)
        );
    }

    #[tokio::test]
    async fn a_folder_that_is_gone_is_missing_and_a_saved_one_is_checked_at_start() {
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app();
        app.intent(Intent::ChooseFolder {
            path: t.root.join("nowhere").display().to_string(),
        })
        .await
        .unwrap();
        until(&app, |s| matches!(s.client.check, CheckView::FolderMissing)).await;
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
        // The next run checks the saved folder by itself.
        let again = t.app();
        until(&again, checked(LevelView::Quick)).await;
    }

    #[tokio::test]
    async fn a_download_runs_in_its_terminal_then_checks_every_file() {
        let t = Testbed::new(&[("a.bin", 1000), ("bin64/b.dll", 10)]);
        let app = t.app_with_fakes(FakeDd::WritesFiles { code: 0 });
        let folder = t.root.join("download");
        app.intent(Intent::Download {
            folder: folder.display().to_string(),
        })
        .await
        .unwrap();
        let s = until(&app, checked(LevelView::Full)).await;
        assert_eq!(s.client.folder.as_deref(), Some(folder.to_str().unwrap()));
        assert!(s.job.is_none());
    }

    #[tokio::test]
    async fn a_repair_fetches_only_the_failing_files() {
        let t = Testbed::new(&[("a.bin", 1000), ("bin64/b.dll", 10)]);
        let app = t.app_with_fakes(FakeDd::WritesFiles { code: 0 });
        let refused = app.intent(Intent::Repair).await.unwrap_err();
        assert_eq!(refused.to_string(), words::NOTHING_TO_REPAIR);
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
        std::fs::write(t.good.join("bin64/b.dll"), b"x").unwrap();
        app.intent(Intent::Check {
            level: LevelView::Full,
        })
        .await
        .unwrap();
        until(&app, |s| {
            matches!(s.client.check, CheckView::NeedsRepair { .. })
        })
        .await;
        app.intent(Intent::Repair).await.unwrap();
        until(&app, checked(LevelView::Full)).await;
        assert_eq!(t.fake_dd_filelist(), vec!["bin64/b.dll"]);
    }

    #[tokio::test]
    async fn a_failed_download_and_a_closed_window_say_so() {
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app_with_fakes(FakeDd::Exits { code: 5 });
        app.intent(Intent::Download {
            folder: t.root.join("d").display().to_string(),
        })
        .await
        .unwrap();
        let s = until(&app, |s| s.notice.is_some()).await;
        assert_eq!(
            s.notice.unwrap().text,
            "The download stopped: DepotDownloader ended with 5. Try again."
        );
        let app = t.app_with_fakes(FakeDd::Hangs);
        app.intent(Intent::Download {
            folder: t.root.join("e").display().to_string(),
        })
        .await
        .unwrap();
        until(&app, |s| matches!(s.job, Some(JobView::Downloading { .. }))).await;
        t.kill_fake_terminal_session().await;
        let s = until(&app, |s| s.job.is_none()).await;
        assert_eq!(s.notice.unwrap().text, words::WINDOW_CLOSED);
    }

    #[tokio::test]
    async fn downloading_works_on_linux_only_and_one_job_runs_at_a_time() {
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app_on(Os::Windows);
        assert_eq!(app.snapshot().client.manual_command, None);
        let err = app
            .intent(Intent::Download {
                folder: "/x".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), words::DOWNLOAD_LINUX_ONLY);
        let app = t.app_with_fakes(FakeDd::Hangs);
        let command = app.snapshot().client.manual_command.unwrap();
        assert!(command.contains("Games/SoLO"), "{command}");
        app.intent(Intent::Download {
            folder: t.root.join("f").display().to_string(),
        })
        .await
        .unwrap();
        let err = app
            .intent(Intent::Check {
                level: LevelView::Quick,
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), words::BUSY);
        for busy in [
            Intent::Download {
                folder: t.root.join("g").display().to_string(),
            },
            Intent::ChooseFolder {
                path: t.good.display().to_string(),
            },
        ] {
            let err = app.intent(busy).await.unwrap_err();
            assert_eq!(err.to_string(), words::BUSY);
        }
        app.intent(Intent::CancelDownload).await.unwrap();
        let s = until(&app, |s| s.job.is_none()).await;
        let notice = s.notice.unwrap();
        assert_eq!(
            (notice.text.as_str(), notice.error),
            (words::DOWNLOAD_CANCELLED, false)
        );
    }

    #[tokio::test]
    async fn a_stopped_check_keeps_the_last_result_and_says_so() {
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app();
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        let before = until(&app, checked(LevelView::Quick)).await.client.check;
        // A check that ends because it was stopped found nothing.
        let found = app.checked(
            LevelView::Full,
            &t.good,
            Err(anyhow::Error::new(crate::verify::Stopped)),
        );
        assert_eq!(found, None);
        let s = app.snapshot();
        assert_eq!(s.client.check, before);
        let notice = s.notice.unwrap();
        assert_eq!(
            (notice.text.as_str(), notice.error),
            (words::CHECK_STOPPED, false)
        );
    }
}
