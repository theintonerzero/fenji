//! The player's copy (launcher spec §3.5, §3.6; L2, L3, L11; plan LP6; Amendment A, LA4 and
//! LA5): its checks, and DepotDownloader's downloads and repairs. DepotDownloader runs hidden, as
//! the launcher's own child, and the player signs in to Steam by scanning the code the launcher
//! draws from its output; a player without the Steam Mobile App runs the command the launcher
//! shows, with their own login, in their own terminal (L3). Either way the sign-in is Steam's,
//! never Fenji's. One job runs at a time. A check runs on a blocking thread.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use super::{
    App, CheckView, CodeView, FileProblem, IntentError, JobView, LevelView, Notice, State, now_ms,
    refused, words,
};
use crate::depot::{self, ChildRun, RunState};
use crate::manifest::Manifest;
use crate::settings::Os;
use crate::tools;
use crate::verify::{self, Level, Problem, Progress, Report, Stopped};

/// How often a running check or fetch publishes its progress, at most.
const PUBLISH_EVERY: Duration = Duration::from_millis(100);
/// How often a download's run folder is read.
const POLL_EVERY: Duration = Duration::from_millis(500);
/// How long a cancelled download has after its `SIGTERM` before `SIGKILL` ends it (the final
/// review of #429, Minor 7).
const STOP_GRACE: Duration = Duration::from_secs(3);
/// How long a download has to end as the launcher exits, after its `SIGTERM`.
const EXIT_GRACE: Duration = Duration::from_secs(2);
const DEPOT_DOWNLOADER: &str = "DepotDownloader";

/// The player's copy as this run knows it.
pub(super) struct Client {
    pub(super) check: CheckView,
    pub(super) job: Option<JobView>,
    /// The running check's stop flag.
    stop: Option<Arc<AtomicBool>>,
    /// The running download, once it has started.
    run: Option<ChildRun>,
    /// Whether the player cancelled the running download.
    cancelled: bool,
    /// DepotDownloader's `-filelist` of the files the last check found wrong, once written, for
    /// the repair's command in the player's own terminal (#348).
    repair_list: Option<PathBuf>,
    /// Whether the chosen folder is in a Steam library, which Steam looks after (Amendment A, LA2).
    pub(super) in_steam: bool,
}

impl Default for Client {
    fn default() -> Self {
        Client {
            check: CheckView::None,
            job: None,
            stop: None,
            run: None,
            cancelled: false,
            repair_list: None,
            in_steam: false,
        }
    }
}

impl Client {
    /// The repair's file list, while the copy needs repair.
    pub(super) fn repair_list(&self) -> Option<&Path> {
        match self.check {
            CheckView::NeedsRepair { .. } => self.repair_list.as_deref(),
            _ => None,
        }
    }
}

/// A check about to run: where, against which files, how deep, and its stop flag.
pub(super) struct CheckJob {
    folder: PathBuf,
    manifest: Manifest,
    level: LevelView,
    /// Files hashed in full before every file has `level`'s check: those the last check found
    /// wrong, when this one is quick (spec §3.5; #348).
    deep: Vec<String>,
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

/// The game's folder stays as it is while the game starts or runs: the start launches the folder
/// Play's check passed (#367), and the game runs from it (#348). A check, which only reads, may run.
fn not_playing(st: &State) -> Result<(), IntentError> {
    if super::play::playing(st) {
        return Err(refused(words::WHILE_PLAYING));
    }
    Ok(())
}

/// `work`, or `None` as soon as `cancelled` says so, looked at every `every`: a fetch dropped
/// midway leaves only its `.part` file, which the next fetch writes over (`tools::ensure`).
pub(super) async fn or_cancelled<T>(
    work: impl std::future::Future<Output = T>,
    cancelled: impl Fn() -> bool,
    every: Duration,
) -> Option<T> {
    let watch = async {
        while !cancelled() {
            tokio::time::sleep(every).await;
        }
    };
    tokio::select! {
        done = work => Some(done),
        () = watch => None,
    }
}

fn waiting_for_steam(repair: bool, code: Option<CodeView>) -> JobView {
    JobView::Downloading {
        repair,
        percent: None,
        file: None,
        waiting_for_steam: true,
        code,
    }
}

impl App {
    /// ChooseFolder: saves the folder, then checks it quickly (spec L11).
    pub(super) fn choose_folder(&self, path: String) -> Result<(), IntentError> {
        if path.trim().is_empty() {
            return Err(refused(words::NO_FOLDER));
        }
        let in_steam = crate::steam::in_library(Path::new(&path), &self.inner.steam_roots);
        let job = self.change(|st| {
            not_playing(st)?;
            free(st)?;
            st.settings.client = Some(PathBuf::from(&path));
            st.client.in_steam = in_steam;
            self.save(st);
            self.begin_check(st, LevelView::Quick)
        })?;
        self.run_check(job);
        Ok(())
    }

    pub(super) fn check(&self, level: LevelView) -> Result<(), IntentError> {
        let job = self.change(|st| self.begin_check(st, level))?;
        self.run_check(job);
        Ok(())
    }

    /// The copy Steam installed, looked for once as the launcher starts, and whether the saved
    /// folder is in a Steam library (Amendment A, LA1 and LA2): a few small files, off the start.
    pub(super) fn find_steam(&self) {
        let app = self.clone();
        self.inner.rt.spawn_blocking(move || {
            let roots = &app.inner.steam_roots;
            let found = crate::steam::find(roots);
            let saved = app.inner.lock().settings.client.clone();
            let in_steam = (saved.as_deref()).is_some_and(|f| crate::steam::in_library(f, roots));
            app.change(|st| {
                st.steam = found;
                // Unless the player chose another folder meanwhile.
                if st.settings.client == saved {
                    st.client.in_steam = in_steam;
                }
            });
        });
    }

    /// The saved folder's quick check when the launcher starts (spec L11).
    pub(super) fn check_at_start(&self) {
        let job = self.change(|st| {
            st.settings.client.as_ref()?;
            self.begin_check(st, LevelView::Quick).ok()
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

    /// Marks a check of the saved folder as the running job: every file at `level`. A quick check
    /// of a copy whose last check found files wrong hashes those in full first, so a file still
    /// wrong with the right size can't pass it: after a repair (spec §3.5), by Fenji or by hand,
    /// and before Play (#348).
    fn begin_check(&self, st: &mut State, level: LevelView) -> Result<CheckJob, IntentError> {
        free(st)?;
        let deep = match (level, &st.client.check) {
            (LevelView::Quick, CheckView::NeedsRepair { files, .. }) => {
                files.iter().map(|f| f.path.clone()).collect()
            }
            _ => Vec::new(),
        };
        let folder = st
            .settings
            .client
            .clone()
            .ok_or_else(|| refused(words::NO_FOLDER))?;
        let manifest = self.inner.cfg.manifest.clone();
        let stop = Arc::new(AtomicBool::new(false));
        st.client.stop = Some(stop.clone());
        st.client.job = Some(JobView::Checking {
            level: if deep.is_empty() {
                level
            } else {
                LevelView::Full
            },
            files_done: 0,
            files_total: if deep.is_empty() {
                manifest.entries.len()
            } else {
                deep.len()
            },
            percent: 0.0,
        });
        Ok(CheckJob {
            folder,
            manifest,
            level,
            deep,
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

    /// Checks the saved folder now, as a job, and gives back what the check found and the folder
    /// it checked: `None` when it was stopped or failed (its notice says which).
    pub(super) async fn check_here(
        &self,
        level: LevelView,
    ) -> Result<Option<(CheckView, PathBuf)>, IntentError> {
        let job = self.change(|st| self.begin_check(st, level))?;
        let folder = job.folder.clone();
        Ok(self.run_check_here(job).await.map(|found| (found, folder)))
    }

    /// Runs the check on a blocking thread, publishing its progress, then its result: the files
    /// in `deep` hashed in full, then every file at the job's level, as one report.
    async fn run_check_here(&self, job: CheckJob) -> Option<CheckView> {
        let CheckJob {
            folder,
            manifest,
            level,
            deep,
            stop,
        } = job;
        let (reporter, looked) = (self.clone(), folder.clone());
        let report = tokio::task::spawn_blocking(move || {
            let throttle = Throttle::new();
            let progress = |level: LevelView| {
                let (reporter, throttle) = (&reporter, &throttle);
                move |p: Progress| {
                    if throttle.due() {
                        reporter.checking(level, p);
                    }
                }
            };
            let mut report = verify::Report::default();
            if !deep.is_empty() {
                let repaired = Manifest {
                    entries: (manifest.entries.iter())
                        .filter(|e| deep.contains(&e.path))
                        .cloned()
                        .collect(),
                };
                let full = progress(LevelView::Full);
                report = verify::verify_with(&looked, &repaired, Level::Full, &full, &stop)?;
            }
            let all =
                verify::verify_with(&looked, &manifest, level.into(), &progress(level), &stop)?;
            report.merge(all);
            Ok(report)
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
    /// manifest lists had the check, so every one counts toward `fine`.
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
                    // The list for a repair in the player's own terminal (#348).
                    st.client.repair_list = None;
                    if !failing.is_empty() {
                        let data = &self.inner.cfg.data_dir;
                        let list = data.join("repair-files.txt");
                        let written = std::fs::create_dir_all(data)
                            .and_then(|()| crate::depot::write_file_list(&list, &failing));
                        match written {
                            Ok(()) => st.client.repair_list = Some(list),
                            Err(e) => warn!(error = %e, "couldn't list the files to repair"),
                        }
                    }
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
        // Steam looks after its libraries (Amendment A, LA2). Settings refuses such a folder too,
        // but one it took before 0.5.3 may be saved (#376).
        let roots = &self.inner.steam_roots;
        if crate::steam::in_library(Path::new(&folder), roots) {
            return Err(refused(if crate::steam::find(roots).is_some() {
                words::IN_STEAM_LIBRARY
            } else {
                words::IN_STEAM_LIBRARY_NO_COPY
            }));
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
            // Steam repairs a copy in its library (Amendment A, LA2).
            if st.client.in_steam {
                return Err(refused(words::REPAIR_IN_STEAM));
            }
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
        not_playing(&self.inner.lock())?;
        let repair = files.is_some();
        self.change(|st| {
            not_playing(st)?;
            free(st)?;
            st.client.cancelled = false;
            st.client.job = Some(match cfg.depot_downloader {
                Some(_) => waiting_for_steam(repair, None),
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
            .spawn(async move { app.downloading(folder, files).await });
        Ok(())
    }

    async fn downloading(&self, folder: PathBuf, files: Option<Vec<String>>) {
        let repair = files.is_some();
        // Fetching DepotDownloader ends at once on Cancel, not when the fetch does (#348).
        let cancelled = || self.inner.lock().client.cancelled;
        let dd = match or_cancelled(self.depot_downloader(), cancelled, POLL_EVERY).await {
            Some(Ok(dd)) => dd,
            Some(Err(text)) => return self.download_ended(text, true),
            None => return self.download_ended(words::DOWNLOAD_CANCELLED.into(), false),
        };
        if cancelled() {
            return self.download_ended(words::DOWNLOAD_CANCELLED.into(), false);
        }
        let job = depot::Job {
            dd,
            folder: folder.clone(),
            files: files.clone(),
        };
        // Hidden, signing in by the code the launcher draws (LA4).
        let started = {
            // A fork while a test writes a script would make that script busy to exec.
            #[cfg(test)]
            let _spawn = crate::test_support::spawning();
            ChildRun::start(&job, &self.run_dir(), self.inner.cfg.env.clone())
        };
        let run = match started {
            Ok(run) => run,
            Err(e) => {
                warn!(error = %e, "DepotDownloader didn't start");
                return self.download_ended(words::DEPOT_DIDNT_START.into(), true);
            }
        };
        self.change(|st| {
            st.client.run = Some(run.clone());
            st.client.job = Some(waiting_for_steam(repair, None));
        });
        let mut tick = tokio::time::interval(POLL_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // When Cancel came: one that outlives its SIGTERM is killed after STOP_GRACE.
        let mut cancelled_at: Option<Instant> = None;
        loop {
            tick.tick().await;
            let cancelled = self.inner.lock().client.cancelled;
            if cancelled {
                // Again on each look: its process group may not have existed at the first.
                let at = *cancelled_at.get_or_insert_with(Instant::now);
                let _ = if at.elapsed() >= STOP_GRACE {
                    run.kill()
                } else {
                    run.cancel()
                };
            }
            let (text, error) = match run.state() {
                RunState::Waiting => {
                    self.change(|st| st.client.job = Some(waiting_for_steam(repair, None)));
                    continue;
                }
                RunState::SignIn { code } => {
                    let code = CodeView::from(&code);
                    self.change(|st| st.client.job = Some(waiting_for_steam(repair, Some(code))));
                    continue;
                }
                RunState::Running { percent, file } => {
                    self.change(|st| {
                        st.client.job = Some(JobView::Downloading {
                            repair,
                            percent: Some(percent),
                            file: Some(file),
                            waiting_for_steam: false,
                            code: None,
                        });
                    });
                    continue;
                }
                RunState::Ended { code: 0 } => return self.downloaded(folder, files),
                _ if cancelled => (words::DOWNLOAD_CANCELLED.to_string(), false),
                RunState::Ended { code } => (words::download_stopped(code), true),
            };
            return self.download_ended(text, error);
        }
    }

    /// DepotDownloader's program: the configured one, or else fetched once into the data folder.
    async fn depot_downloader(&self) -> Result<PathBuf, String> {
        match &self.inner.cfg.depot_downloader {
            Some(dd) => Ok(dd.clone()),
            None => {
                let dd = self
                    .fetch_tool(tools::depot_downloader(), DEPOT_DOWNLOADER)
                    .await?;
                self.change(|st| st.depot_downloader = Some(dd.clone()));
                Ok(dd)
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
        tools::ensure(
            &tool,
            &self.inner.cfg.data_dir.join("tools"),
            &self.fetching(name),
        )
        .await
        .map_err(|e| words::tool_failed(name, &e.to_string()))
    }

    /// A fetch's progress, as the job `Fetching { tool: name }` shows it, at most every
    /// [`PUBLISH_EVERY`]. With no job, it takes the slot: Play's GE-Proton shows only once a
    /// download comes. Another job is left as it is.
    pub(super) fn fetching(
        &self,
        name: &'static str,
    ) -> impl Fn(u64, Option<u64>) + Send + Sync + '_ {
        let throttle = Throttle::new();
        move |done: u64, total: Option<u64>| {
            if !throttle.due() {
                return;
            }
            let percent = total
                .filter(|&t| t > 0)
                .map(|t| (done as f64 * 100.0 / t as f64) as f32);
            self.change(|st| {
                let ours = match &st.client.job {
                    None => true,
                    Some(JobView::Fetching { tool, .. }) => tool == name,
                    Some(_) => false,
                };
                if ours {
                    st.client.job = Some(JobView::Fetching {
                        tool: name.into(),
                        percent,
                    });
                }
            });
        }
    }

    /// A fresh run folder: `<data dir>/depot/<unix ms>`. The runs before it go first (#348): one
    /// job runs at a time, so none of them is in use, and the last stays until the next run, for
    /// what it says if it failed.
    fn run_dir(&self) -> PathBuf {
        let base = self.inner.cfg.data_dir.join("depot");
        if let Ok(old) = std::fs::read_dir(&base) {
            for entry in old.flatten() {
                let run = entry.file_name();
                if run
                    .to_str()
                    .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
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
    /// it is checked, after a repair too.
    fn downloaded(&self, folder: PathBuf, files: Option<Vec<String>>) {
        info!("DepotDownloader finished");
        let job = self.change(|st| {
            (st.client.job, st.client.run) = (None, None);
            if files.is_none() {
                st.settings.client = Some(folder);
                // A download never goes into a Steam library.
                st.client.in_steam = false;
                self.save(st);
            }
            // After a download, every file in full. After a repair, a quick check, which hashes
            // the repaired files in full first (spec §3.5), and which the record names: a full
            // check would hash the whole copy, 100 GiB for the pinned build (#348).
            let level = if files.is_none() {
                LevelView::Full
            } else {
                LevelView::Quick
            };
            self.begin_check(st, level)
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
            match &st.client.job {
                // Play's own fetches, umu-launcher's for a first start and GE-Proton's for any,
                // wait on Cancel start's flag: cancelling one is cancelling the start (#367).
                Some(JobView::Fetching { tool, .. })
                    if [super::play::UMU_LAUNCHER, super::play::GE_PROTON]
                        .contains(&tool.as_str())
                        && matches!(st.game, super::GameView::Starting { .. }) =>
                {
                    st.start_cancelled = true;
                    return None;
                }
                Some(JobView::Fetching { .. } | JobView::Downloading { .. }) => {}
                _ => return None,
            }
            st.client.cancelled = true;
            st.client.run.clone()
        });
        if let Some(run) = run {
            let _ = run.cancel();
        }
    }

    /// Ends a running download as the launcher exits (the final review of #429, I3):
    /// DepotDownloader runs hidden, as the launcher's own child, with no window of its own, so
    /// nothing else would stop it. `SIGTERM`, then `SIGKILL` after [`EXIT_GRACE`]: it waits that
    /// long at most. The shell calls it on exit; what a launcher that ended without it left, the
    /// next one ends ([`depot::end_left`]).
    pub fn end_downloads(&self) {
        let run = self.change(|st| {
            st.client.cancelled = true;
            st.client.run.clone()
        });
        if let Some(run) = run {
            run.stop(EXIT_GRACE);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use super::super::{
        App, AppConfig, CheckView, CodeView, Intent, JobView, LevelView, SteamCopyView, words,
    };
    #[cfg(target_os = "linux")]
    use super::{EXIT_GRACE, STOP_GRACE};
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
        /// Signs in as DepotDownloader 3.4.0 did in Task 11's rehearsal, its own lines up to its
        /// download (`depot::reading_tests::SIGN_IN`), holding the code a while for the player
        /// to scan, then writes the files as `WritesFiles` with code 0.
        DrawsCode,
        /// Ignores `SIGTERM`, notes its pid in `dd-pid`, and waits.
        #[cfg(target_os = "linux")]
        IgnoresTerm,
    }

    /// The Steam account the fake DepotDownloader signs in to, by the code it draws: the one the
    /// rehearsal's fixtures name in the account's place.
    const ACCOUNT: &str = "lin_yue_on_steam";

    /// Every event any thread logs from the first call on, at every level, as text: the test
    /// binary's own subscriber, set once. A hidden run reads its output on threads of its own.
    fn logged() -> &'static Mutex<Vec<u8>> {
        static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());
        static SET: std::sync::Once = std::sync::Once::new();
        struct Writer;
        impl std::io::Write for Writer {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                LOG.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        SET.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .with_writer(|| Writer)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("no other test sets the subscriber");
            // A callsite seen before, with no subscriber, is wanted from now on.
            tracing::callsite::rebuild_interest_cache();
        });
        &LOG
    }

    /// The code the fake DepotDownloader draws: the rehearsal's, as its fixture has it.
    fn drawn_code() -> Vec<Vec<bool>> {
        crate::depot::reading_tests::code(37, 1)
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
                umu_run: None,
                ge_proton: None,
                gamemoderun: None,
            }
        }

        fn app(&self) -> App {
            self.app_on(Os::Linux)
        }

        fn app_on(&self, os: Os) -> App {
            App::start(self.config(os), tokio::runtime::Handle::current())
        }

        /// An app whose DepotDownloader is a fake.
        fn app_with_fakes(&self, dd: FakeDd) -> App {
            let n = self.fakes.fetch_add(1, Ordering::Relaxed);
            let draws = matches!(dd, FakeDd::DrawsCode);
            let dd = if draws {
                FakeDd::WritesFiles { code: 0 }
            } else {
                dd
            };
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
                #[cfg(target_os = "linux")]
                FakeDd::IgnoresTerm => format!(
                    "trap '' TERM\necho $$ > '{}'\nexec sleep 600",
                    self.root.join("dd-pid").display()
                ),
                FakeDd::DrawsCode => unreachable!("drawn as WritesFiles above"),
            };
            let body = if draws {
                // Up to its code, then from its sign-in, where it names the account
                // (Steam3Session.cs), to its download, which the files' own lines take over.
                let lines: Vec<&str> = crate::depot::reading_tests::SIGN_IN.lines().collect();
                let signed_in = lines
                    .iter()
                    .position(|l| l.starts_with("Success!"))
                    .unwrap();
                let download = lines
                    .iter()
                    .position(|l| l.starts_with("Pre-allocating"))
                    .unwrap();
                let (code, steam) = (self.root.join("dd-code"), self.root.join("dd-steam"));
                std::fs::write(&code, lines[..signed_in].join("\n") + "\n").unwrap();
                std::fs::write(&steam, lines[signed_in..download].join("\n") + "\n").unwrap();
                format!(
                    "cat '{}'\nsleep 1.5\ncat '{}'\n{body}",
                    code.display(),
                    steam.display()
                )
            } else {
                body
            };
            let mut cfg = self.config(Os::Linux);
            {
                let _spawn = spawning();
                cfg.depot_downloader = Some(script(&self.root, &format!("dd-{n}"), &body));
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
    }

    fn checked(level: LevelView) -> impl Fn(&super::super::Snapshot) -> bool {
        move |s| matches!(s.client.check, CheckView::Verified { level: l, .. } if l == level)
    }

    #[tokio::test]
    async fn a_fetch_that_finds_depot_downloader_installed_puts_it_in_the_snapshot() {
        // #417's review: the place a fetch finds is the snapshot's from then on, with no restart.
        let t = Testbed::new(&[("a.bin", 10)]);
        let cfg = t.config(Os::Linux);
        let tool = crate::tools::depot_downloader();
        let home = t
            .root
            .join("data/tools")
            .join(format!("{}-{}", tool.name, tool.version));
        let app = App::start(cfg, tokio::runtime::Handle::current());
        assert!(app.snapshot().client.manual.unwrap().get.is_some());
        {
            let _spawn = spawning();
            std::fs::create_dir_all(&home).unwrap();
            let digest: String = tool.sha256.iter().map(|b| format!("{b:02x}")).collect();
            std::fs::write(home.join(".sha256"), digest).unwrap();
            let program = home.join(&tool.member);
            std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        app.intent(Intent::Download {
            folder: t.root.join("f").display().to_string(),
        })
        .await
        .unwrap();
        until(&app, |s| {
            s.client.manual.as_ref().is_some_and(|m| m.get.is_none())
        })
        .await;
    }

    #[tokio::test]
    async fn cancel_download_of_umu_launchers_fetch_while_no_game_starts_is_the_tabs_own() {
        // The other half of #367's guard: with no start, the tab's cancel is the tab's.
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app();
        app.change(|st| {
            st.client.job = Some(JobView::Fetching {
                tool: "umu-launcher".into(),
                percent: None,
            });
        });
        app.intent(Intent::CancelDownload).await.unwrap();
        let st = app.inner.lock();
        assert!(st.client.cancelled, "the tab's own cancel");
        assert!(!st.start_cancelled, "no start to cancel");
    }

    #[tokio::test]
    async fn cancel_download_during_plays_own_fetch_cancels_the_start() {
        // #367: the Game tab's Cancel download, while Play fetched umu-launcher for a first
        // start, set the tab's own flag, which that fetch never looks at. GE-Proton's fetch, at
        // any start, is Play's too (launcher spec Amendment B, LB1).
        for tool in ["umu-launcher", "GE-Proton"] {
            let t = Testbed::new(&[("a.bin", 10)]);
            let app = t.app();
            app.change(|st| {
                st.game = super::super::GameView::Starting { first_start: true };
                st.client.job = Some(JobView::Fetching {
                    tool: tool.into(),
                    percent: None,
                });
            });
            app.intent(Intent::CancelDownload).await.unwrap();
            let st = app.inner.lock();
            assert!(
                st.start_cancelled,
                "{tool}: the start sees it as Cancel start"
            );
            assert!(
                !st.client.cancelled,
                "{tool}: the tab had no download of its own"
            );
        }
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
    async fn a_download_runs_hidden_then_checks_every_file() {
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
    async fn a_download_signs_in_by_the_code_drawn_in_the_launcher() {
        let t = Testbed::new(&[("a.bin", 1000), ("bin64/b.dll", 10)]);
        let app = t.app_with_fakes(FakeDd::DrawsCode);
        let folder = t.root.join("download");
        app.intent(Intent::Download {
            folder: folder.display().to_string(),
        })
        .await
        .unwrap();
        let s = until(&app, |s| {
            matches!(&s.job, Some(JobView::Downloading { code: Some(_), .. }))
        })
        .await;
        let Some(JobView::Downloading {
            code: Some(code),
            waiting_for_steam,
            ..
        }) = s.job
        else {
            unreachable!()
        };
        let dark: String = drawn_code()
            .concat()
            .iter()
            .map(|&d| if d { '1' } else { '0' })
            .collect();
        assert_eq!(code, CodeView { size: 37, dark });
        assert!(waiting_for_steam);
        let s = until(&app, checked(LevelView::Full)).await;
        assert_eq!(s.client.folder.as_deref(), Some(folder.to_str().unwrap()));
    }

    #[tokio::test]
    async fn no_snapshot_and_no_log_line_holds_a_code_line_or_the_account_name() {
        // The plan's Task 12, its third test (the final review of #429, I4): DepotDownloader's
        // output gives the code's squares and progress, and nothing else of it is kept, logged or
        // shown: not the account it names once signed in, not a line of the code.
        let log = logged();
        let t = Testbed::new(&[("a.bin", 1000), ("bin64/b.dll", 10)]);
        let app = t.app_with_fakes(FakeDd::DrawsCode);
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let (keep, mut rx) = (seen.clone(), app.subscribe());
        tokio::spawn(async move {
            loop {
                let json = serde_json::to_string(&*rx.borrow_and_update()).unwrap();
                keep.lock().unwrap().push(json);
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
        let folder = t.root.join("download");
        app.intent(Intent::Download {
            folder: folder.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, |s| {
            matches!(&s.job, Some(JobView::Downloading { code: Some(_), .. }))
        })
        .await;
        until(&app, checked(LevelView::Full)).await;
        let dark: String = drawn_code()
            .concat()
            .iter()
            .map(|&d| if d { '1' } else { '0' })
            .collect();
        let shown = seen.lock().unwrap().clone();
        assert!(
            shown.iter().any(|json| json.contains(&dark)),
            "the code reaches the view as its squares"
        );
        let settings =
            std::fs::read_to_string(t.root.join("config/settings.json")).unwrap_or_default();
        let logs = String::from_utf8_lossy(&log.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned();
        assert!(
            logs.contains("DepotDownloader finished"),
            "the log was caught"
        );
        // Nor its other lines, the rehearsal's own: the licences, the CellID, the request code.
        let others = [
            "licenses for account",
            "CellID",
            "request code",
            "Got depot key",
        ];
        for text in shown.iter().chain([&settings, &logs]) {
            assert!(
                !text.contains(ACCOUNT) && !text.contains('█'),
                "DepotDownloader's output got out: {text}"
            );
            for other in others {
                assert!(!text.contains(other), "{other} got out: {text}");
            }
        }
        assert!(!logs.contains(&dark), "the code reached the log");
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
        // The repaired file in full, then every file's quick check (spec §3.5).
        until(&app, checked(LevelView::Quick)).await;
        assert_eq!(t.fake_dd_filelist(), vec!["bin64/b.dll"]);
    }

    #[tokio::test]
    async fn a_repair_checks_its_files_fully_then_every_file_quickly() {
        // #348: the check after a repair said every file had been checked when it had hashed only
        // the repaired ones; then it hashed the whole copy, 100 GiB for the pinned build, where
        // spec §3.5 asks for the repaired files to be checked fully again. Now they are, every file
        // has the quick check, and the record says quick.
        let t = Testbed::new(&[("bin64/a.dll", 10), ("data/c.pak", 100)]);
        let app = t.app_with_fakes(FakeDd::WritesFiles { code: 0 });
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
        std::fs::write(t.good.join("data/c.pak"), b"short").unwrap();
        app.intent(Intent::Check {
            level: LevelView::Quick,
        })
        .await
        .unwrap();
        until(&app, |s| {
            matches!(s.client.check, CheckView::NeedsRepair { .. })
        })
        .await;
        // What comes back has the right size and the wrong bytes, outside bin64: only a full
        // check of the repaired file can tell.
        let mut bytes = std::fs::read(t.pristine.join("data/c.pak")).unwrap();
        bytes[0] ^= 0xff;
        std::fs::write(t.pristine.join("data/c.pak"), &bytes).unwrap();
        app.intent(Intent::Repair).await.unwrap();
        let s = until(&app, |s| s.job.is_none()).await;
        let CheckView::NeedsRepair { files, .. } = &s.client.check else {
            panic!("after the first repair: {:?}", s.client.check)
        };
        assert_eq!(
            files
                .iter()
                .map(|f| (f.path.as_str(), f.problem))
                .collect::<Vec<_>>(),
            [("data/c.pak", Problem::WrongHash)]
        );
        bytes[0] ^= 0xff;
        std::fs::write(t.pristine.join("data/c.pak"), &bytes).unwrap();
        app.intent(Intent::Repair).await.unwrap();
        let s = until(&app, |s| s.job.is_none()).await;
        assert!(
            matches!(
                s.client.check,
                CheckView::Verified {
                    level: LevelView::Quick,
                    ..
                }
            ),
            "after the second repair: {:?}",
            s.client.check
        );
    }

    #[tokio::test]
    async fn a_quick_check_hashes_the_files_found_wrong_in_full() {
        // #348's final review: after a repair by hand (DepotDownloader in the player's own
        // terminal, or files copied from a Gameforge install), a quick check saw only sizes outside
        // bin64, so a file still wrong with the right size passed, and Play could start with it.
        let t = Testbed::new(&[("bin64/a.dll", 10), ("data/c.pak", 100)]);
        let app = t.app();
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
        let mut bytes = std::fs::read(t.good.join("data/c.pak")).unwrap();
        bytes[0] ^= 0xff;
        std::fs::write(t.good.join("data/c.pak"), &bytes).unwrap();
        app.intent(Intent::Check {
            level: LevelView::Full,
        })
        .await
        .unwrap();
        until(&app, |s| {
            matches!(s.client.check, CheckView::NeedsRepair { .. })
        })
        .await;
        app.intent(Intent::Check {
            level: LevelView::Quick,
        })
        .await
        .unwrap();
        let s = until(&app, |s| s.job.is_none()).await;
        let CheckView::NeedsRepair { files, .. } = &s.client.check else {
            panic!(
                "the quick check passed a file it knew was wrong: {:?}",
                s.client.check
            )
        };
        assert_eq!(files[0].path, "data/c.pak");
        // Fixed by hand, it passes the next quick check.
        bytes[0] ^= 0xff;
        std::fs::write(t.good.join("data/c.pak"), &bytes).unwrap();
        app.intent(Intent::Check {
            level: LevelView::Quick,
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
    }

    #[tokio::test]
    async fn depot_downloaders_place_is_looked_up_at_start_not_on_each_change() {
        // #367: every snapshot, under the state lock, read the tools folder for it.
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app();
        assert!(app.snapshot().client.manual.unwrap().get.is_some());
        // Installed behind the launcher's back, as a fetch would leave it.
        let tool = crate::tools::depot_downloader();
        let home = t
            .root
            .join("data/tools")
            .join(format!("{}-{}", tool.name, tool.version));
        std::fs::create_dir_all(&home).unwrap();
        let digest: String = tool.sha256.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(home.join(".sha256"), digest).unwrap();
        let program = home.join(&tool.member);
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        assert!(
            app.snapshot().client.manual.unwrap().get.is_some(),
            "a change reads no folder"
        );
        // The next launcher finds it.
        assert_eq!(t.app().snapshot().client.manual.unwrap().get, None);
    }

    #[tokio::test]
    async fn the_repairs_manual_command_names_a_file_list_fenji_wrote() {
        // #348's final review: Repair's way without a terminal showed the download's command, with
        // no -validate and no list of files, which can't repair a copy DepotDownloader made.
        let t = Testbed::new(&[("bin64/a.dll", 10), ("data/c.pak", 100)]);
        let app = t.app();
        let manual = app.snapshot().client.manual.unwrap();
        assert_eq!(manual.repair, None);
        // Fenji hasn't fetched DepotDownloader: the dialog says where to get it.
        assert_eq!(
            manual.get.unwrap().url,
            crate::tools::depot_downloader().url
        );
        app.intent(Intent::ChooseFolder {
            path: t.good.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
        std::fs::write(t.good.join("data/c.pak"), b"short").unwrap();
        app.intent(Intent::Check {
            level: LevelView::Quick,
        })
        .await
        .unwrap();
        let s = until(&app, |s| {
            matches!(s.client.check, CheckView::NeedsRepair { .. })
        })
        .await;
        let repair = s.client.manual.unwrap().repair.unwrap();
        let list = t.root.join("data/repair-files.txt");
        assert!(
            repair.ends_with(&format!(" -filelist '{}' -validate", list.display())),
            "{repair}"
        );
        assert_eq!(std::fs::read_to_string(&list).unwrap(), "data/c.pak\n");
        // Once the copy checks out, there's nothing to repair.
        std::fs::copy(t.pristine.join("data/c.pak"), t.good.join("data/c.pak")).unwrap();
        app.intent(Intent::Check {
            level: LevelView::Quick,
        })
        .await
        .unwrap();
        let s = until(&app, checked(LevelView::Quick)).await;
        assert_eq!(s.client.manual.unwrap().repair, None);
    }

    /// A Steam folder in the testbed's home holding the build, as Steam installs it: the list of
    /// its libraries, the game's manifest at `build`, and the game's files.
    fn steam_install(t: &Testbed, build: &str) -> PathBuf {
        let steam = t.root.join("home/.local/share/Steam");
        let apps = steam.join("steamapps");
        let game = apps.join("common/Swords of Legends Online");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(
            apps.join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
                steam.display()
            ),
        )
        .unwrap();
        std::fs::write(
            apps.join("appmanifest_1418100.acf"),
            format!(
                "\"AppState\" {{ \"installdir\" \"Swords of Legends Online\" \"buildid\" \"{build}\" }}"
            ),
        )
        .unwrap();
        for e in &t.manifest.entries {
            let to = game.join(&e.path);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(t.pristine.join(&e.path), to).unwrap();
        }
        game
    }

    #[tokio::test]
    async fn a_copy_steam_installed_is_offered_and_used_where_it_is() {
        // The launcher spec's Amendment A, LA1: a player whose copy Steam installed uses it.
        let t = Testbed::new(&[("bin64/a.dll", 10), ("data/c.pak", 100)]);
        let game = steam_install(&t, "10015727");
        let app = t.app();
        let s = until(&app, |s| s.client.steam.is_some()).await;
        assert_eq!(
            s.client.steam,
            Some(SteamCopyView {
                folder: game.display().to_string(),
                build: "10015727".into(),
                pinned: true,
            })
        );
        assert!(!s.client.in_steam);
        app.intent(Intent::ChooseFolder {
            path: game.display().to_string(),
        })
        .await
        .unwrap();
        let s = until(&app, checked(LevelView::Quick)).await;
        assert!(s.client.in_steam);
        assert_eq!(s.client.folder.as_deref(), game.to_str());
    }

    #[tokio::test]
    async fn settings_refuses_a_download_folder_in_a_steam_library() {
        // #376 (the redesign's final review, Minor 16): Settings took a download folder inside a
        // Steam library, which only Download refused, later, in words that didn't say where to
        // change it, and offered Steam's copy where Steam had none.
        let t = Testbed::new(&[("bin64/a.dll", 10), ("data/c.pak", 100)]);
        let game = steam_install(&t, "10015727");
        let app = t.app();
        let before = app.snapshot().client.default_folder;
        let into = game.with_file_name("Another Copy").display().to_string();
        assert_eq!(
            app.intent(Intent::SetDownloadFolder { path: into.clone() })
                .await
                .unwrap_err()
                .to_string(),
            words::STEAM_FOLDER_IN_SETTINGS
        );
        assert_eq!(app.snapshot().client.default_folder, before);
        assert_eq!(
            app.snapshot().notice.map(|n| n.text).as_deref(),
            Some(words::STEAM_FOLDER_IN_SETTINGS)
        );
        // A download folder Settings took before 0.5.3: Download names Settings, and Steam's copy.
        assert_eq!(
            app.intent(Intent::Download {
                folder: into.clone()
            })
            .await
            .unwrap_err()
            .to_string(),
            words::IN_STEAM_LIBRARY
        );
        // Where Steam has no copy, Download doesn't offer one.
        std::fs::remove_file(
            game.parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("appmanifest_1418100.acf"),
        )
        .unwrap();
        assert_eq!(
            app.intent(Intent::Download { folder: into })
                .await
                .unwrap_err()
                .to_string(),
            words::IN_STEAM_LIBRARY_NO_COPY
        );
    }

    #[tokio::test]
    async fn fenji_never_writes_into_a_steam_library() {
        // LA2: Steam looks after what's in its libraries.
        let t = Testbed::new(&[("bin64/a.dll", 10), ("data/c.pak", 100)]);
        let game = steam_install(&t, "10015727");
        let app = t.app_with_fakes(FakeDd::WritesFiles { code: 0 });
        until(&app, |s| s.client.steam.is_some()).await;
        // A new folder inside the library is Steam's too.
        let into = game.with_file_name("Another Copy");
        assert_eq!(
            app.intent(Intent::Download {
                folder: into.display().to_string(),
            })
            .await
            .unwrap_err()
            .to_string(),
            words::IN_STEAM_LIBRARY
        );
        app.intent(Intent::ChooseFolder {
            path: game.display().to_string(),
        })
        .await
        .unwrap();
        until(&app, checked(LevelView::Quick)).await;
        std::fs::write(game.join("data/c.pak"), b"short").unwrap();
        app.intent(Intent::Check {
            level: LevelView::Quick,
        })
        .await
        .unwrap();
        let s = until(&app, |s| {
            matches!(s.client.check, CheckView::NeedsRepair { .. })
        })
        .await;
        assert_eq!(s.client.manual.unwrap().repair, None);
        assert_eq!(
            app.intent(Intent::Repair).await.unwrap_err().to_string(),
            words::REPAIR_IN_STEAM
        );
    }

    #[tokio::test]
    async fn a_cancel_ends_the_work_it_waits_on_at_once() {
        // #348: Cancel didn't stop "Getting DepotDownloader…" until the fetch had ended.
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        use super::or_cancelled;
        let flag = Arc::new(AtomicBool::new(false));
        let set = {
            let flag = flag.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                flag.store(true, Ordering::SeqCst);
            })
        };
        let started = std::time::Instant::now();
        let every = Duration::from_millis(10);
        let cancelled = || flag.load(Ordering::SeqCst);
        let got = or_cancelled(std::future::pending::<u8>(), cancelled, every).await;
        assert_eq!(got, None);
        assert!(started.elapsed() < Duration::from_secs(2));
        set.await.unwrap();
        // Work that ends first is what comes back.
        assert_eq!(or_cancelled(async { 7 }, || false, every).await, Some(7));
    }

    #[tokio::test]
    async fn only_the_last_depot_run_keeps_its_folder() {
        // #348: each DepotDownloader run left its folder in <data dir>/depot for good. The last
        // one stays, for what it says if it failed; a new run clears those before it.
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app_with_fakes(FakeDd::WritesFiles { code: 0 });
        for folder in ["d1", "d2", "d3"] {
            app.intent(Intent::Download {
                folder: t.root.join(folder).display().to_string(),
            })
            .await
            .unwrap();
            until(&app, |s| s.job.is_none()).await;
        }
        let runs = std::fs::read_dir(t.root.join("data/depot"))
            .unwrap()
            .count();
        assert_eq!(runs, 1);
    }

    #[tokio::test]
    async fn a_failed_download_says_so() {
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
            "The download stopped (code 5). Try again."
        );
    }

    /// The pid the fake DepotDownloader noted, once it has.
    #[cfg(target_os = "linux")]
    async fn dd_pid(t: &Testbed) -> i32 {
        let file = t.root.join("dd-pid");
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(pid) = std::fs::read_to_string(&file)
                    .ok()
                    .and_then(|text| text.trim().parse().ok())
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the fake DepotDownloader never started")
    }

    /// Whether process `pid` lives, and isn't a zombie waiting to be reaped: Linux's `/proc`.
    #[cfg(target_os = "linux")]
    fn alive(pid: i32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
            && !std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
                stat.rsplit(')')
                    .next()
                    .is_some_and(|r| r.trim_start().starts_with('Z'))
            })
    }

    // Linux's, as downloads are, and `alive` is.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn cancel_ends_even_a_download_that_ignores_sigterm() {
        // The final review of #429, Minor 7: SIGKILL after STOP_GRACE.
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app_with_fakes(FakeDd::IgnoresTerm);
        app.intent(Intent::Download {
            folder: t.root.join("d").display().to_string(),
        })
        .await
        .unwrap();
        let pid = dd_pid(&t).await;
        app.intent(Intent::CancelDownload).await.unwrap();
        let s = tokio::time::timeout(
            STOP_GRACE + Duration::from_secs(5),
            until(&app, |s| s.job.is_none()),
        )
        .await
        .expect("the download outlived its cancel");
        assert_eq!(s.notice.unwrap().text, words::DOWNLOAD_CANCELLED);
        assert!(!alive(pid), "DepotDownloader lives on");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn closing_the_launcher_ends_its_hidden_download() {
        // The final review of #429, I3: no window of its own, so nothing else would stop it; even
        // one that ignores SIGTERM ends within EXIT_GRACE.
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app_with_fakes(FakeDd::IgnoresTerm);
        app.intent(Intent::Download {
            folder: t.root.join("d").display().to_string(),
        })
        .await
        .unwrap();
        let pid = dd_pid(&t).await;
        let started = std::time::Instant::now();
        app.end_downloads();
        assert!(
            started.elapsed() < EXIT_GRACE + Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        let until_gone = std::time::Instant::now() + Duration::from_secs(2);
        while alive(pid) && std::time::Instant::now() < until_gone {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(pid), "DepotDownloader lives on after the launcher");
        // With nothing running, there's nothing to wait for.
        let idle = t.app();
        let started = std::time::Instant::now();
        idle.end_downloads();
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[tokio::test]
    async fn downloading_works_on_linux_only_and_one_job_runs_at_a_time() {
        let t = Testbed::new(&[("a.bin", 10)]);
        let app = t.app_on(Os::Windows);
        assert_eq!(app.snapshot().client.manual, None);
        let err = app
            .intent(Intent::Download {
                folder: "/x".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), words::DOWNLOAD_LINUX_ONLY);
        let app = t.app_with_fakes(FakeDd::Hangs);
        let command = app.snapshot().client.manual.unwrap().download;
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
