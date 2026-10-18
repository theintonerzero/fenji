//! DepotDownloader as the launcher's own child (launcher spec §3.6; Amendment A, LA4; L21; plan
//! LP6). The player signs in to Steam by scanning the code it draws, which the launcher draws
//! again; a player without the Steam Mobile App runs the command the launcher shows, with their own
//! login, in their own terminal (L3, LA5). No Steam login or password ever passes through Fenji's
//! code (ground rule 7). Unix only, like Play.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::appimage;

/// The pinned build's arguments (spec §3.6; the plan's Global Constraints).
const PINNED_ARGS: [&str; 10] = [
    "-app",
    "1418100",
    "-depot",
    "1418101",
    "-manifest",
    "5002037851744157048",
    "-os",
    "windows",
    "-osarch",
    "64",
];

/// One run of DepotDownloader: the program, the folder it downloads into, and for a repair the
/// manifest paths to fetch again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub dd: PathBuf,
    pub folder: PathBuf,
    pub files: Option<Vec<String>>,
}

/// `path`, single-quoted for a POSIX shell, and for fish: each piece between its backslashes in
/// quotes of its own, joined by `\\` outside them, since fish reads a backslash inside single
/// quotes as an escape (#367). With no backslash, one piece: the plain single-quoted form.
fn quoted(path: &Path) -> String {
    path.to_string_lossy()
        .split('\\')
        .map(|piece| format!("'{}'", piece.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join("\\\\")
}

/// The command a player without the Steam app runs in their own terminal, on one line, their own
/// login left for them to type. It runs `dd`, the DepotDownloader Fenji fetched, by its path;
/// without one, the player gets the pinned release (the dialog says where) and runs it from its
/// folder, made executable first (#348). A repair names `list`, the failing files as Fenji listed
/// them ([`write_file_list`]), with `-validate`, as the launcher's own run does (plan LP6).
pub fn manual_command(dd: Option<&Path>, folder: &Path, list: Option<&Path>) -> String {
    let program = match dd {
        Some(dd) => quoted(dd),
        None => "chmod +x ./DepotDownloader && ./DepotDownloader".to_string(),
    };
    let mut command = format!(
        "{program} {} -username <your Steam login> -dir {}",
        PINNED_ARGS.join(" "),
        quoted(folder)
    );
    if let Some(list) = list {
        command.push_str(&format!(" -filelist {} -validate", quoted(list)));
    }
    command
}

/// DepotDownloader's `-filelist`: one path a line, as the manifest names them.
pub fn write_file_list(path: &Path, files: &[String]) -> std::io::Result<()> {
    let mut text = files.join("\n");
    text.push('\n');
    std::fs::write(path, text)
}

/// One of DepotDownloader's progress lines, `{0,6:#00.00}% {path}`: the percentage and the path.
pub fn parse_progress(line: &str) -> Option<(f32, String)> {
    let (number, rest) = line.trim_start().split_once('%')?;
    let percent: f32 = number.trim().parse().ok()?;
    let path = rest.strip_prefix(' ')?.trim();
    ((0.0..=100.0).contains(&percent) && !path.is_empty()).then(|| (percent, path.to_string()))
}

/// The line DepotDownloader 3.4.0 prints before each sign-in code it draws, the first and each
/// that takes its place ("The QR code has changed:" comes first then) (launcher spec Amendment A,
/// LA4).
pub const QR_HEADER: &str = "Use the Steam Mobile App to sign in with this QR code:";

/// The fewest and the most modules on a side a drawn code can have: QR versions 1 to 40, 21 to
/// 177 modules, and up to 4 more each side for its quiet zone.
const CODE_SIDES: std::ops::RangeInclusive<usize> = 21..=185;

/// A sign-in code as DepotDownloader drew it, to draw again as squares: Fenji never decodes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrCode {
    /// Modules on a side, its quiet zone included.
    pub size: usize,
    /// Whether each module is dark, row by row.
    pub dark: Vec<bool>,
}

/// What a line of DepotDownloader's tells the launcher.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A whole sign-in code, drawn since its header.
    Code(QrCode),
    /// A progress line: the percentage and the file.
    Progress(f32, String),
}

/// Reads DepotDownloader's output a line at a time. It keeps only a code it's still reading; any
/// other line is used at once or dropped, never kept: the account name DepotDownloader prints
/// after a sign-in among them (ground rule 7).
#[derive(Debug, Default)]
pub struct Reader {
    /// The rows of a code being read since its header.
    code: Option<Vec<Vec<bool>>>,
}

impl Reader {
    /// What `line` tells, if anything.
    pub fn line(&mut self, line: &str) -> Option<Event> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == QR_HEADER {
            self.code = Some(Vec::new());
            return None;
        }
        if let Some(rows) = &mut self.code {
            // A row that isn't the code's breaks it: what was read goes, unread.
            match modules(line) {
                Some(row)
                    if rows
                        .first()
                        .map_or(CODE_SIDES.contains(&row.len()), |first| {
                            first.len() == row.len()
                        }) =>
                {
                    rows.push(row);
                    if rows.len() == rows[0].len() {
                        let rows = self.code.take().unwrap_or_default();
                        return Some(Event::Code(QrCode {
                            size: rows.len(),
                            dark: rows.concat(),
                        }));
                    }
                    return None;
                }
                _ => self.code = None,
            }
        }
        parse_progress(line).map(|(percent, file)| Event::Progress(percent, file))
    }
}

/// A drawn row's modules: `██` dark and two spaces light, and nothing else.
fn modules(line: &str) -> Option<Vec<bool>> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() || !chars.len().is_multiple_of(2) {
        return None;
    }
    chars
        .chunks(2)
        .map(|pair| match pair {
            ['█', '█'] => Some(true),
            [' ', ' '] => Some(false),
            _ => None,
        })
        .collect()
}

/// Where a run is.
#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    /// Started; the player is signing in to Steam, or nothing has come yet.
    Waiting,
    /// A hidden run's sign-in code, for the player to scan with the Steam Mobile App: the last
    /// one DepotDownloader drew, which takes the place of any before it.
    SignIn {
        code: QrCode,
    },
    Running {
        percent: f32,
        file: String,
    },
    Ended {
        code: i32,
    },
}

/// The longest line read whole. A longer one is skipped: a drawn code's row is at most 185
/// modules of two three-byte characters, and a progress line a path.
const MAX_LINE: usize = 16 * 1024;

/// DepotDownloader as the launcher's own child, signing in by QR code (launcher spec Amendment A,
/// LA4; L21), in a process group of its own. Its input is closed. What it prints goes through a
/// [`Reader`]: the code it draws, to draw again as squares, and its progress; every other line is
/// dropped unread, and its error output too, so nothing it prints is logged or kept. What it
/// saves goes as it ends ([`OWN_DATA`]). Unix only, like Play.
#[derive(Debug, Clone)]
pub struct ChildRun {
    seen: Arc<Mutex<Seen>>,
    /// The child's own process group: its pid.
    pgid: i32,
    /// Its own data folder, in its run's folder ([`OWN_DATA`]).
    data: PathBuf,
}

/// What a hidden run has shown so far.
#[derive(Debug, Default)]
struct Seen {
    code: Option<QrCode>,
    progress: Option<(f32, String)>,
    exit: Option<i32>,
}

fn lock(seen: &Mutex<Seen>) -> std::sync::MutexGuard<'_, Seen> {
    seen.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ChildRun {
    /// Starts DepotDownloader on `job` with `-qr`, its environment `env` as it is outside an
    /// AppImage ([`appimage::outside`]) but for its own data folder ([`OWN_DATA`]); that folder,
    /// and a repair's file list, go into a fresh `run_dir`.
    pub fn start(
        job: &Job,
        run_dir: &Path,
        env: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> std::io::Result<ChildRun> {
        let mut dirs = std::fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
        let data = run_dir.join(OWN_DATA);
        dirs.create(&data)?;
        let mut cmd = std::process::Command::new(&job.dd);
        cmd.args(PINNED_ARGS)
            .arg("-qr")
            .arg("-dir")
            .arg(&job.folder);
        if let Some(files) = &job.files {
            let list = run_dir.join("files.txt");
            write_file_list(&list, files)?;
            cmd.arg("-filelist").arg(list).arg("-validate");
        }
        cmd.env_clear()
            .envs(appimage::outside(env))
            .env("XDG_DATA_HOME", &data)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn()?;
        let pgid = i32::try_from(child.id()).unwrap_or(0);
        // For the next launcher, should this one end without stopping it ([`end_left`]).
        std::fs::write(
            run_dir.join(LEFT),
            format!("{pgid} {}\n", started_at(pgid).unwrap_or(0)),
        )?;
        let seen = Arc::new(Mutex::new(Seen::default()));
        if let Some(mut errors) = child.stderr.take() {
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut errors, &mut std::io::sink());
            });
        }
        if let Some(out) = child.stdout.take() {
            let seen = seen.clone();
            std::thread::spawn(move || read_lines(out, &seen));
        }
        let (ended, saved) = (seen.clone(), data.clone());
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
            // Before the run is seen to end, so that nothing it saved outlives it.
            let _ = std::fs::remove_dir_all(saved);
            lock(&ended).exit = Some(code);
        });
        Ok(ChildRun { seen, pgid, data })
    }

    /// Where the run is now: its end, else its progress once the download has begun, else the
    /// code to scan, else waiting.
    pub fn state(&self) -> RunState {
        let seen = lock(&self.seen);
        if let Some(code) = seen.exit {
            RunState::Ended { code }
        } else if let Some((percent, file)) = &seen.progress {
            RunState::Running {
                percent: *percent,
                file: file.clone(),
            }
        } else if let Some(code) = &seen.code {
            RunState::SignIn { code: code.clone() }
        } else {
            RunState::Waiting
        }
    }

    /// Asks the run to end: `SIGTERM` to its process group, whatever DepotDownloader started
    /// with it.
    pub fn cancel(&self) -> std::io::Result<()> {
        signal_group(self.pgid, false)
    }

    /// Ends the run at once: `SIGKILL` to its process group, for one that outlived its `SIGTERM`.
    pub fn kill(&self) -> std::io::Result<()> {
        signal_group(self.pgid, true)
    }

    /// Ends the run and waits for it: `SIGTERM`, then `SIGKILL` to whatever of it is left after
    /// `grace`. As the launcher exits: DepotDownloader has no window of its own, so nothing else
    /// would stop it (the final review of #429, I3).
    pub fn stop(&self, grace: Duration) {
        let _ = self.cancel();
        if !self.ended_within(grace) {
            let _ = self.kill();
            // Killed, it's gone once its waiter has reaped it.
            self.ended_within(KILLED_GONE);
        }
        // Its waiter removes its data as it ends, but the launcher may exit first (#443).
        let _ = std::fs::remove_dir_all(&self.data);
    }

    /// Whether the run's process group is gone, looked at until `limit` has passed.
    fn ended_within(&self, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        while group_alive(self.pgid) {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }
}

/// How long a killed run's group has to be gone, once its waiter reaps it.
const KILLED_GONE: Duration = Duration::from_secs(1);

/// The file in a run's folder naming its process group and when its leader started, for
/// [`end_left`].
const LEFT: &str = "pgid";

/// DepotDownloader's own data folder in a run's folder, its `XDG_DATA_HOME`, where .NET keeps its
/// isolated storage. DepotDownloader saves its settings there, the account and Steam token of
/// every sign-in among them, whatever its flags (3.4.0's `Steam3Session.cs`). Fenji never has it
/// use them again (no `-remember-password`, LA4), so the folder goes as the run ends (#443).
const OWN_DATA: &str = "data";

/// When process `pid` started, in clock ticks since boot (`/proc/<pid>/stat`'s 22nd field), which
/// tells it from a later process given the same number. None where there's no `/proc`.
fn started_at(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name, in parentheses, may hold spaces and parentheses itself.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(19)?.parse().ok()
}

/// Ends what a launcher that didn't stop its runs left in `runs`, its runs' folders: each run
/// whose process group still lives, led by the very process it started, is killed. A number the
/// system has since given another process is left alone. What every run there saved goes: no run
/// of this launcher's has started yet (#443). Only its runs' own folders, named by their start
/// as the sweep before each run takes them, never through a link.
pub fn end_left(runs: &Path) {
    let Ok(entries) = std::fs::read_dir(runs) else {
        return;
    };
    for run in entries.filter_map(Result::ok) {
        let named = run
            .file_name()
            .to_str()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !named || !run.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        end_noted(&run.path());
        let _ = std::fs::remove_dir_all(run.path().join(OWN_DATA));
    }
}

/// Kills the run that run folder `run`'s note names, if it's still that very run, and removes
/// the note.
fn end_noted(run: &Path) {
    let left = run.join(LEFT);
    let Ok(text) = std::fs::read_to_string(&left) else {
        return;
    };
    let mut words = text.split_whitespace();
    let (Some(Ok(pgid)), Some(Ok(at))) = (
        words.next().map(str::parse::<i32>),
        words.next().map(str::parse::<u64>),
    ) else {
        return;
    };
    if pgid > 0 && started_at(pgid) == Some(at) {
        let _ = signal_group(pgid, true);
    }
    let _ = std::fs::remove_file(left);
}

/// Reads a hidden run's output to its end, a line at a time, into `seen`.
fn read_lines(out: impl std::io::Read, seen: &Mutex<Seen>) {
    use std::io::{BufRead, Read};
    let mut out = std::io::BufReader::new(out);
    let mut reader = Reader::default();
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut out)
            .take(MAX_LINE as u64)
            .read_until(b'\n', &mut line)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.last() != Some(&b'\n') && line.len() == MAX_LINE {
            // Too long to be one of the lines it means: the rest of it goes unread.
            if out.skip_until(b'\n').is_err() {
                return;
            }
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        match reader.line(text.strip_suffix('\n').unwrap_or(&text)) {
            Some(Event::Code(code)) => lock(seen).code = Some(code),
            Some(Event::Progress(percent, file)) => {
                // Downloading: the player has signed in.
                let mut seen = lock(seen);
                seen.code = None;
                seen.progress = Some((percent, file));
            }
            None => {}
        }
    }
}

#[cfg(unix)]
fn group_alive(pgid: i32) -> bool {
    use rustix::process::{Pid, test_kill_process_group};
    Pid::from_raw(pgid).is_some_and(|pid| match test_kill_process_group(pid) {
        Ok(()) => true,
        // It exists, but isn't ours to signal.
        Err(e) => e != rustix::io::Errno::SRCH,
    })
}

#[cfg(not(unix))]
fn group_alive(_: i32) -> bool {
    false
}

/// `SIGTERM`, or with `kill` `SIGKILL`, to process group `pgid`; one that has ended is fine.
#[cfg(unix)]
fn signal_group(pgid: i32, kill: bool) -> std::io::Result<()> {
    use rustix::process::{Pid, Signal, kill_process_group};
    let Some(pid) = Pid::from_raw(pgid) else {
        return Ok(());
    };
    let signal = if kill { Signal::KILL } else { Signal::TERM };
    match kill_process_group(pid, signal) {
        Err(e) if e != rustix::io::Errno::SRCH => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(not(unix))]
fn signal_group(_: i32, _: bool) -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::test_support::spawning;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn progress_lines_are_read_and_the_rest_ignored() {
        assert_eq!(
            parse_progress(" 42.13% data/x.bin"),
            Some((42.13, "data/x.bin".into()))
        );
        assert_eq!(
            parse_progress("100.00% bin64/gujianol.exe"),
            Some((100.0, "bin64/gujianol.exe".into()))
        );
        for line in [
            "Got session token!",
            "Total downloaded: 3 bytes",
            " 12.00% ",
            "150.00% x",
            "abc% x",
        ] {
            assert_eq!(parse_progress(line), None, "{line}");
        }
    }

    /// #367: in fish, a backslash inside single quotes escapes the next quote, so a folder named with
    /// a backslash ended the one-line command's quote early. Backslashes go outside the quotes now,
    /// where sh, bash, zsh and fish all read `\\` and `\'` as themselves.
    #[test]
    fn a_path_with_a_backslash_is_quoted_so_fish_reads_it_as_posix_shells_do() {
        assert_eq!(quoted(Path::new("/games/SoLO")), "'/games/SoLO'");
        assert_eq!(quoted(Path::new("/g/it's")), r"'/g/it'\''s'");
        assert_eq!(quoted(Path::new(r"/g/my\'game")), r"'/g/my'\\''\''game'");
        assert_eq!(quoted(Path::new(r"/g/end\")), r"'/g/end'\\''");
        // The shells it starts would hold another test's fresh script open (ETXTBSY).
        let _spawn = spawning();
        for shell in ["sh", "bash", "zsh"] {
            let Ok(probe) = std::process::Command::new(shell)
                .arg("-c")
                .arg("true")
                .status()
            else {
                continue;
            };
            assert!(probe.success());
            for path in [
                "/games/Swords of Legends Online",
                "/g/it's",
                r"/g/my\'game",
                r"/g/end\",
                r"/g/a\b\\c",
            ] {
                let out = std::process::Command::new(shell)
                    .arg("-c")
                    .arg(format!("printf '%s' {}", quoted(Path::new(path))))
                    .output()
                    .unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout),
                    path,
                    "{shell}: {path}"
                );
            }
        }
    }

    #[test]
    fn the_manual_command_leaves_the_login_to_the_player() {
        let c = manual_command(None, Path::new("/home/p/Games/It's SoLO"), None);
        assert!(c.contains(
            "./DepotDownloader -app 1418100 -depot 1418101 -manifest 5002037851744157048"
        ));
        assert!(
            c.contains("-username <your Steam login>")
                && c.contains("-dir '/home/p/Games/It'\\''s SoLO'")
        );
    }

    #[test]
    fn the_manual_command_names_a_depot_downloader_the_player_has() {
        // #348: it named a DepotDownloader the player had never got. Fenji's own copy, if it
        // fetched one, is used by its path; otherwise the player gets one (the dialog says where)
        // and runs it from its folder, made executable first. Either way it's one line.
        let dd = Path::new("/home/p/.local/share/fenji-launcher/tools/DepotDownloader");
        let c = manual_command(Some(dd), Path::new("/home/p/Games/SoLO"), None);
        assert!(
            c.starts_with(
                "'/home/p/.local/share/fenji-launcher/tools/DepotDownloader' -app 1418100"
            ),
            "{c}"
        );
        let c = manual_command(None, Path::new("/home/p/Games/SoLO"), None);
        assert!(
            c.starts_with("chmod +x ./DepotDownloader && ./DepotDownloader -app 1418100"),
            "{c}"
        );
        assert!(!c.contains('\n') && !c.contains('#'), "{c}");
    }

    #[test]
    fn a_repairs_manual_command_validates_the_files_fenji_listed() {
        // #348's final review: without -validate, DepotDownloader trusts what it saved about a copy
        // it made, and fetches nothing (plan LP6).
        let c = manual_command(
            None,
            Path::new("/home/p/Games/SoLO"),
            Some(Path::new(
                "/home/p/.local/share/fenji-launcher/repair-files.txt",
            )),
        );
        assert!(
            c.ends_with(
                " -filelist '/home/p/.local/share/fenji-launcher/repair-files.txt' -validate"
            ),
            "{c}"
        );
        assert!(!c.contains('\n'), "{c}");
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("list");
        write_file_list(&list, &["bin64/a.dll".into(), "data/c.pak".into()]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&list).unwrap(),
            "bin64/a.dll\ndata/c.pak\n"
        );
    }

    use super::reading_tests::{CANCEL, EXPIRED, EXPIRED_ERRORS, REPAIRED, SIGN_IN, code, drawn};

    /// A fake DepotDownloader 3.4.0: it notes its arguments, one a line, and whether its input
    /// was closed, in `dir/args`, then does `body`.
    fn fake_dd(dir: &Path, body: &str) -> PathBuf {
        let args = dir.join("args");
        let args = args.display();
        script(
            dir,
            "DepotDownloader",
            &format!(
                "printf '%s\\n' \"$@\" > '{args}'\nif read -r _; then echo open >> '{args}'; else echo closed >> '{args}'; fi\n{body}"
            ),
        )
    }

    /// Shell lines that print `lines`, one each.
    fn printing(lines: &[String]) -> String {
        lines
            .iter()
            .map(|line| format!("printf '%s\\n' '{line}'\n"))
            .collect()
    }

    fn start_child(dd: &Path, folder: &Path, files: Option<Vec<String>>, run: &Path) -> ChildRun {
        let job = Job {
            dd: dd.to_path_buf(),
            folder: folder.to_path_buf(),
            files,
        };
        ChildRun::start(&job, run, std::env::vars_os()).unwrap()
    }

    fn wait_child(run: &ChildRun, done: impl Fn(&RunState) -> bool) -> RunState {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let s = run.state();
            if done(&s) || std::time::Instant::now() > until {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn sign_in(code: &[Vec<bool>]) -> RunState {
        RunState::SignIn {
            code: QrCode {
                size: code.len(),
                dark: code.concat(),
            },
        }
    }

    #[test]
    fn a_hidden_run_shows_each_code_then_its_progress_then_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second) = (code(29, 1), code(29, 2));
        let body = format!(
            "echo 'Logging in with QR code...'\necho '{QR_HEADER}'\n{}sleep 0.4\necho ''\necho 'The QR code has changed:'\necho '{QR_HEADER}'\n{}sleep 0.4\necho 'Success! Next time you can login with -username someone -remember-password instead of -qr.'\necho ' 50.00% bin64/gujianol.exe'\nsleep 0.4\necho '100.00% data/ui.pak'\nexit 0",
            printing(&drawn(&first)),
            printing(&drawn(&second))
        );
        let run = {
            let _spawn = spawning();
            let dd = fake_dd(dir.path(), &body);
            start_child(&dd, &dir.path().join("SoLO"), None, &dir.path().join("run"))
        };
        assert_eq!(wait_child(&run, |s| *s == sign_in(&first)), sign_in(&first));
        assert_eq!(
            wait_child(&run, |s| *s == sign_in(&second)),
            sign_in(&second)
        );
        assert_eq!(
            wait_child(&run, |s| matches!(s, RunState::Running { .. })),
            RunState::Running {
                percent: 50.0,
                file: "bin64/gujianol.exe".into()
            }
        );
        assert_eq!(
            wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 0 }
        );
    }

    /// A fake DepotDownloader's lines that play `output` back a line at a time, pausing after each
    /// empty line and before its end so each code is seen, then write `errors` to its error output
    /// and exit with `exit`, or with none wait to be stopped.
    fn playing(dir: &Path, output: &str, errors: &str, exit: Option<i32>) -> String {
        let (out, err) = (dir.join("output"), dir.join("errors"));
        std::fs::write(&out, output).unwrap();
        std::fs::write(&err, errors).unwrap();
        let end = exit.map_or("sleep 30".into(), |code| format!("exit {code}"));
        format!(
            "while IFS= read -r line; do printf '%s\\n' \"$line\"; [ -n \"$line\" ] || sleep 0.3; done < '{}'\nsleep 0.3\ncat '{}' >&2\n{end}",
            out.display(),
            err.display()
        )
    }

    #[test]
    fn depot_downloaders_own_output_takes_a_hidden_run_from_its_codes_to_its_end() {
        // Task 11's rehearsal, played back (tests/fixtures/depot-downloader-3.4.0).
        let start = |output: &str, errors: &str, exit: Option<i32>| {
            let dir = tempfile::tempdir().unwrap();
            let run = {
                let _spawn = spawning();
                let dd = fake_dd(dir.path(), &playing(dir.path(), output, errors, exit));
                start_child(&dd, &dir.path().join("SoLO"), None, &dir.path().join("run"))
            };
            (dir, run)
        };
        let ended = |run: &ChildRun| wait_child(run, |s| matches!(s, RunState::Ended { .. }));
        let codes = [code(37, 1), code(37, 2), code(37, 3)];
        // Scanned: the code, then the download, then exit 0.
        let (_dir, run) = start(SIGN_IN, "", Some(0));
        let first = sign_in(&codes[0]);
        assert_eq!(wait_child(&run, |s| *s == first), first);
        assert_eq!(
            wait_child(&run, |s| matches!(s, RunState::Running { .. })),
            RunState::Running {
                percent: 100.0,
                file: REPAIRED.into()
            }
        );
        assert_eq!(ended(&run), RunState::Ended { code: 0 });
        // Nobody scans: each new code in turn, DepotDownloader's error, then exit 1.
        let (_dir, run) = start(EXPIRED, EXPIRED_ERRORS, Some(1));
        for code in &codes {
            let shown = sign_in(code);
            assert_eq!(wait_child(&run, |s| *s == shown), shown);
        }
        assert_eq!(ended(&run), RunState::Ended { code: 1 });
        // Cancelled while the code waits.
        let (_dir, run) = start(CANCEL, "", None);
        assert_eq!(wait_child(&run, |s| *s == first), first);
        run.cancel().unwrap();
        assert!(matches!(ended(&run), RunState::Ended { .. }));
    }

    /// #329: DepotDownloader starts outside the AppImage, as the terminal's script did.
    #[test]
    fn a_hidden_run_gets_the_environment_outside_the_appimage() {
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("seen");
        let run = {
            let _spawn = spawning();
            let dd = fake_dd(dir.path(), &format!("env | sort > '{}'", seen.display()));
            let job = Job {
                dd,
                folder: dir.path().join("game"),
                files: None,
            };
            let env = [
                ("APPDIR", "/tmp/.mount_FenjiAb12"),
                ("PATH", "/tmp/.mount_FenjiAb12/usr/bin/:/usr/bin:/bin"),
                ("PYTHONHOME", "/tmp/.mount_FenjiAb12/usr/"),
                // A player's own gives way to the run's (#443).
                ("XDG_DATA_HOME", "/home/p/.local/share"),
            ]
            .map(|(k, v)| (OsString::from(k), OsString::from(v)));
            ChildRun::start(&job, &dir.path().join("run"), env).unwrap()
        };
        assert_eq!(
            wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 0 }
        );
        let text = std::fs::read_to_string(&seen).unwrap();
        let vars: Vec<&str> = text
            .lines()
            .filter(|l| !["PWD=", "SHLVL=", "_="].iter().any(|p| l.starts_with(p)))
            .collect();
        // And its own data folder (#443).
        let data = format!(
            "XDG_DATA_HOME={}",
            dir.path().join("run").join(OWN_DATA).display()
        );
        assert_eq!(vars, ["PATH=/usr/bin:/bin", data.as_str()]);
    }

    /// Where a fake that saves a sign-in ([`reading_tests::saving_sign_in`]) saved it, once it has.
    fn saved_in(saved: &Path) -> PathBuf {
        let until = Instant::now() + Duration::from_secs(5);
        while !saved.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        let own = std::fs::read_to_string(saved).expect("the run's own data folder, to save in");
        PathBuf::from(own.trim())
    }

    /// #443: DepotDownloader saves the sign-in's account and Steam token after every one,
    /// whatever its flags, in its own settings under `$XDG_DATA_HOME`. A hidden run's is a folder
    /// of its own in the run's folder, gone as soon as the run ends, finished or cancelled; the
    /// run's folder stays until the next run (#348).
    #[test]
    fn a_hidden_runs_own_data_goes_as_soon_as_it_ends() {
        for cancelled in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (run_dir, saved) = (dir.path().join("run"), dir.path().join("saved"));
            let end = if cancelled { "exec sleep 30" } else { "exit 0" };
            let run = {
                let _spawn = spawning();
                let save = reading_tests::saving_sign_in(dir.path(), &saved);
                let dd = fake_dd(dir.path(), &format!("{save}\n{end}"));
                let files = Some(vec!["bin64/a.dll".into()]);
                start_child(&dd, &dir.path().join("SoLO"), files, &run_dir)
            };
            assert_eq!(saved_in(&saved), run_dir.join(OWN_DATA));
            if cancelled {
                run.cancel().unwrap();
            }
            assert!(matches!(
                wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
                RunState::Ended { .. }
            ));
            assert!(
                !run_dir.join(OWN_DATA).exists(),
                "the sign-in outlived its run (cancelled: {cancelled})"
            );
            assert!(
                run_dir.join("files.txt").exists(),
                "the run's folder stays until the next run"
            );
        }
    }

    /// The final review of #429, I3: a run that ignores `SIGTERM` still ends, `SIGKILL` after the
    /// grace, with everything it started; and its sign-in with it (#443), before the launcher
    /// that stopped it exits.
    #[test]
    fn stop_ends_even_a_run_that_ignores_sigterm() {
        let dir = tempfile::tempdir().unwrap();
        let (run_dir, saved) = (dir.path().join("run"), dir.path().join("saved"));
        let run = {
            let _spawn = spawning();
            let save = reading_tests::saving_sign_in(dir.path(), &saved);
            let body = format!("trap '' TERM\n{save}\necho started\nexec sleep 600");
            let dd = fake_dd(dir.path(), &body);
            start_child(&dd, &dir.path().join("SoLO"), None, &run_dir)
        };
        assert_eq!(saved_in(&saved), run_dir.join(OWN_DATA));
        // While it holds a sign-in, only the player may look in: .NET's own folders are 0755.
        for held in [run_dir.clone(), run_dir.join(OWN_DATA)] {
            let mode = std::fs::metadata(&held).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{held:?}");
        }
        let started = Instant::now();
        run.stop(Duration::from_millis(300));
        assert!(!group_alive(run.pgid), "it lives on after its stop");
        assert!(
            !run_dir.join(OWN_DATA).exists(),
            "the sign-in outlived the stop"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        assert!(matches!(
            wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { .. }
        ));
    }

    /// The final review of #429, I3: a launcher that crashed, or was killed, left its run going;
    /// the next one ends it, and only it. Linux's: the note's start comes from `/proc`, and
    /// downloads are Linux's alone.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_run_a_launcher_left_behind_is_ended_by_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let runs = dir.path().join("depot");
        let run = {
            let _spawn = spawning();
            let dd = fake_dd(dir.path(), "trap '' TERM\nexec sleep 600");
            start_child(&dd, &dir.path().join("SoLO"), None, &runs.join("1"))
        };
        let left = runs.join("1").join(LEFT);
        let own = runs.join("1").join(OWN_DATA);
        assert!(own.is_dir(), "the run has its own data folder");
        let text = std::fs::read_to_string(&left).unwrap();
        // A number given since to another process: its start differs, and it's left alone.
        let (pgid, at) = text.trim().split_once(' ').unwrap();
        std::fs::write(
            &left,
            format!("{pgid} {}\n", at.parse::<u64>().unwrap() + 1),
        )
        .unwrap();
        end_left(&runs);
        // Time for a run wrongly killed to be reaped, and seen to end.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            run.state(),
            RunState::Waiting,
            "another process's number is no run of ours"
        );
        assert!(group_alive(run.pgid));
        // No run of this launcher's is going yet, so what a left one saved goes, whatever its
        // note says (#443).
        assert!(!own.exists(), "a left run's sign-in stayed");
        // The very run: ended.
        std::fs::write(&left, &text).unwrap();
        end_left(&runs);
        let until = Instant::now() + Duration::from_secs(5);
        while group_alive(run.pgid) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!group_alive(run.pgid), "the run it left lives on");
        assert!(!left.exists(), "its note goes with it");
        // A crashed launcher's run, long ended, with nobody left to wait for it: what it saved
        // goes too, its note or none.
        let crashed = runs.join("2").join(OWN_DATA).join("IsolatedStorage");
        std::fs::create_dir_all(&crashed).unwrap();
        std::fs::write(crashed.join("account.config"), "lin_yue_on_steam").unwrap();
        end_left(&runs);
        assert!(
            !runs.join("2").join(OWN_DATA).exists(),
            "a crashed run's sign-in stayed"
        );
        // Only its own runs' folders, named by their start: never through a link, and never a
        // folder it didn't make.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(elsewhere.join(OWN_DATA)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, runs.join("3")).unwrap();
        std::fs::create_dir_all(runs.join("notes").join(OWN_DATA)).unwrap();
        end_left(&runs);
        assert!(elsewhere.join(OWN_DATA).is_dir(), "it went through a link");
        assert!(
            runs.join("notes").join(OWN_DATA).is_dir(),
            "it swept a folder not its own"
        );
        // A folder with no note, or none at all, is nothing to end.
        end_left(&dir.path().join("none"));
    }

    #[test]
    fn a_hidden_run_has_the_pinned_build_the_qr_sign_in_and_no_input() {
        let dir = tempfile::tempdir().unwrap();
        let (folder, run_dir) = (dir.path().join("SoLO"), dir.path().join("run"));
        let run = {
            let _spawn = spawning();
            let dd = fake_dd(dir.path(), "exit 0");
            start_child(&dd, &folder, Some(vec!["bin64/a.dll".into()]), &run_dir)
        };
        assert_eq!(
            wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 0 }
        );
        let list = run_dir.join("files.txt");
        let mut want: Vec<String> = PINNED_ARGS.iter().map(|a| a.to_string()).collect();
        want.extend(["-qr".into(), "-dir".into(), folder.display().to_string()]);
        want.extend([
            "-filelist".into(),
            list.display().to_string(),
            "-validate".into(),
        ]);
        want.push("closed".into());
        let got = std::fs::read_to_string(dir.path().join("args")).unwrap();
        assert_eq!(got.lines().collect::<Vec<_>>(), want);
        assert_eq!(std::fs::read_to_string(&list).unwrap(), "bin64/a.dll\n");
    }

    #[test]
    fn cancel_ends_a_hidden_run_and_everything_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let pid = dir.path().join("sleeper");
        let run = {
            let _spawn = spawning();
            let dd = fake_dd(
                dir.path(),
                &format!("sleep 30 &\necho $! > '{}'\nwait", pid.display()),
            );
            start_child(&dd, &dir.path().join("SoLO"), None, &dir.path().join("run"))
        };
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !pid.exists() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let sleeper: i32 = std::fs::read_to_string(&pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        run.cancel().unwrap();
        assert!(matches!(
            wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { .. }
        ));
        let alive = || {
            rustix::process::Pid::from_raw(sleeper)
                .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
        };
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while alive() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!alive(), "the run's own child outlived the cancel");
    }

    #[test]
    fn a_loud_error_output_never_stalls_a_hidden_run() {
        // Far more than a pipe holds: unread, it would stop the run where it is.
        let dir = tempfile::tempdir().unwrap();
        let run = {
            let _spawn = spawning();
            let dd = fake_dd(
                dir.path(),
                "head -c 2000000 /dev/zero >&2\necho '100.00% data/ui.pak'\nexit 0",
            );
            start_child(&dd, &dir.path().join("SoLO"), None, &dir.path().join("run"))
        };
        assert_eq!(
            wait_child(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 0 }
        );
    }
}

#[cfg(test)]
pub(crate) mod reading_tests {
    use super::*;

    /// A code as DepotDownloader 3.4.0 draws it: QRCoder 1.6.0's `GetLineByLineGraphic(1)`, two
    /// characters a module, `██` dark and two spaces light, its quiet zone included.
    pub(crate) fn drawn(code: &[Vec<bool>]) -> Vec<String> {
        code.iter()
            .map(|row| {
                row.iter()
                    .map(|&dark| if dark { "██" } else { "  " })
                    .collect()
            })
            .collect()
    }

    /// A `size`-module code with a quiet zone of 4 and a pattern inside it, from `seed`.
    pub(crate) fn code(size: usize, seed: usize) -> Vec<Vec<bool>> {
        (0..size)
            .map(|y| {
                (0..size)
                    .map(|x| {
                        let inside = (4..size - 4).contains(&x) && (4..size - 4).contains(&y);
                        inside && (x * 7 + y * 3 + seed) % 5 < 2
                    })
                    .collect()
            })
            .collect()
    }

    /// DepotDownloader 3.4.0's own output, from Task 11's rehearsal (#428): what identifies the
    /// account replaced, and each code `code(37, n)` (tests/fixtures/depot-downloader-3.4.0).
    pub(crate) const SIGN_IN: &str =
        include_str!("../tests/fixtures/depot-downloader-3.4.0/sign-in.txt");
    pub(crate) const EXPIRED: &str =
        include_str!("../tests/fixtures/depot-downloader-3.4.0/expired.txt");
    pub(crate) const EXPIRED_ERRORS: &str =
        include_str!("../tests/fixtures/depot-downloader-3.4.0/expired.stderr.txt");
    pub(crate) const CANCEL: &str =
        include_str!("../tests/fixtures/depot-downloader-3.4.0/cancel.txt");

    /// Where the rehearsal's one file was, as the fixtures name it.
    pub(crate) const REPAIRED: &str = "/home/player/Games/SoLO/bin64/loader.ini";

    /// Shell lines that save a sign-in as DepotDownloader 3.4.0 does after every one, whatever
    /// its flags (`Steam3Session.cs`): the account and its Steam token in `account.config`, in
    /// .NET's isolated store under `$XDG_DATA_HOME`; then `$XDG_DATA_HOME` goes into `saved`.
    /// Only where `$XDG_DATA_HOME` is inside `within`, so no test writes in a real one (#443).
    /// Unix only, like the tests that run it.
    #[cfg(unix)]
    pub(crate) fn saving_sign_in(within: &Path, saved: &Path) -> String {
        format!(
            r#"case "$XDG_DATA_HOME" in '{within}'/*)
  store="$XDG_DATA_HOME/IsolatedStorage/a.b/c.d/Url.e/AssemFiles"
  mkdir -p "$store" && echo 'lin_yue_on_steam eyFakeSteamToken' > "$store/account.config" &&
    echo "$XDG_DATA_HOME" > '{saved}.new' && mv '{saved}.new' '{saved}' ;;
esac"#,
            within = within.display(),
            saved = saved.display(),
        )
    }

    fn read(lines: &[String]) -> Vec<Event> {
        let mut reader = Reader::default();
        lines.iter().filter_map(|line| reader.line(line)).collect()
    }

    fn lines(output: &str) -> Vec<String> {
        output.lines().map(str::to_string).collect()
    }

    #[test]
    fn depot_downloaders_own_sign_in_is_read_as_its_code_then_its_progress() {
        // Task 11's rehearsal: the code, scanned, then a repair of one file.
        assert_eq!(
            read(&lines(SIGN_IN)),
            vec![
                Event::Code(squares(&code(37, 1))),
                Event::Progress(100.0, REPAIRED.into()),
            ]
        );
    }

    #[test]
    fn depot_downloaders_own_new_codes_are_read_in_turn_and_its_failure_is_nothing() {
        // Task 11's rehearsal: nobody scans, a new code twice, then the end; and a cancel.
        let codes: Vec<Event> = (1..=3)
            .map(|n| Event::Code(squares(&code(37, n))))
            .collect();
        assert_eq!(read(&lines(EXPIRED)), codes);
        assert_eq!(read(&lines(EXPIRED_ERRORS)), vec![]);
        assert_eq!(read(&lines(CANCEL)), codes[..1]);
    }

    fn text(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    fn squares(code: &[Vec<bool>]) -> QrCode {
        QrCode {
            size: code.len(),
            dark: code.concat(),
        }
    }

    #[test]
    fn a_drawn_code_is_read_as_its_squares() {
        let first = code(29, 1);
        let got = read(
            &[
                text(&["Logging in with QR code...", QR_HEADER]),
                drawn(&first),
            ]
            .concat(),
        );
        assert_eq!(got, vec![Event::Code(squares(&first))]);
    }

    #[test]
    fn a_changed_code_takes_the_first_ones_place() {
        let (first, second) = (code(29, 1), code(29, 2));
        let got = read(
            &[
                text(&[QR_HEADER]),
                drawn(&first),
                text(&["", "The QR code has changed:", QR_HEADER]),
                drawn(&second),
            ]
            .concat(),
        );
        assert_eq!(
            got,
            vec![Event::Code(squares(&first)), Event::Code(squares(&second))]
        );
    }

    #[test]
    fn rows_that_break_the_square_are_no_code() {
        let good = code(29, 1);
        let mut narrow = drawn(&good);
        // One module fewer: its last two characters, the quiet zone's spaces.
        narrow[10].pop();
        narrow[10].pop();
        let mut odd = drawn(&good);
        odd[3].push(' ');
        let mut letters = drawn(&good);
        letters[12] = letters[12].replacen("██", "XX", 1);
        for bad in [narrow, odd, letters] {
            assert_eq!(
                read(&[text(&[QR_HEADER]), bad.clone()].concat()),
                vec![],
                "{bad:?}"
            );
        }
        // A code cut short, then a whole one: only the whole one.
        let short = drawn(&good)[..20].to_vec();
        let got = read(&[text(&[QR_HEADER]), short, text(&[QR_HEADER]), drawn(&good)].concat());
        assert_eq!(got, vec![Event::Code(squares(&good))]);
        // Too small or too large for any QR code.
        let small = drawn(&code(12, 1));
        assert_eq!(read(&[text(&[QR_HEADER]), small].concat()), vec![]);
        let huge = drawn(&vec![vec![false; 200]; 200]);
        assert_eq!(read(&[text(&[QR_HEADER]), huge].concat()), vec![]);
    }

    #[test]
    fn progress_is_read_and_every_other_line_is_nothing() {
        let got = read(&text(&[
            "Logging in with QR code...",
            "Success! Next time you can login with -username someone -remember-password instead of -qr.",
            "Got depot key for 1418101 result: OK",
            " 12.50% bin64/gujianol.exe",
            "Total downloaded: 1 bytes (2 bytes uncompressed) from 1 depots",
            "100.00% data/ui.pak",
        ]));
        assert_eq!(
            got,
            vec![
                Event::Progress(12.5, "bin64/gujianol.exe".into()),
                Event::Progress(100.0, "data/ui.pak".into()),
            ]
        );
    }

    #[test]
    fn a_windows_line_end_is_no_part_of_a_line() {
        let first = code(29, 1);
        let mut crlf: Vec<String> = drawn(&first).into_iter().map(|l| l + "\r").collect();
        crlf.insert(0, format!("{QR_HEADER}\r"));
        assert_eq!(read(&crlf), vec![Event::Code(squares(&first))]);
    }
}
