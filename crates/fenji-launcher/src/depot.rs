//! DepotDownloader in its own terminal window (launcher spec §3.6; L2, L3; plan LP6). The player
//! signs in to Steam there, by QR code, so no Steam login or password ever passes through Fenji's
//! code (ground rule 7). The window's script leaves the launcher only DepotDownloader's progress
//! lines and its exit status, in files of the run's folder. Unix only, like Play.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::appimage;

/// The script the terminal runs: `sh depot.sh <run folder> <DepotDownloader> <folder> [<file list>]`.
const SCRIPT: &str = include_str!("depot.sh");

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

/// Terminals tried, in order, after `xdg-terminal-exec` and `$TERMINAL`: each one's program and
/// the arguments that come before the command it runs.
pub const TERMINALS: &[(&str, &[&str])] = &[
    ("ptyxis", &["--"]),
    ("kgx", &["--"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("mate-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("alacritty", &["-e"]),
    ("kitty", &[]),
    ("foot", &[]),
    ("wezterm", &["start", "--"]),
    ("x-terminal-emulator", &["-e"]),
    ("xterm", &["-e"]),
];

/// A terminal that runs `program before… sh <script> …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    pub program: PathBuf,
    pub before: Vec<String>,
}

/// The first terminal there is: `xdg-terminal-exec`, then `$TERMINAL` (with `-e`), then
/// [`TERMINALS`]. `var` reads the environment and `on_path` finds a program on `PATH`.
pub fn find_terminal(
    var: &dyn Fn(&str) -> Option<OsString>,
    on_path: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<Terminal> {
    if let Some(program) = on_path("xdg-terminal-exec") {
        return Some(Terminal {
            program,
            before: vec![],
        });
    }
    if let Some(name) = var("TERMINAL").filter(|n| !n.is_empty()) {
        let given = PathBuf::from(&name);
        let program = if given.components().count() > 1 {
            Some(given)
        } else {
            on_path(&name.to_string_lossy())
        };
        if let Some(program) = program {
            return Some(Terminal {
                program,
                before: vec!["-e".into()],
            });
        }
    }
    TERMINALS.iter().find_map(|(name, before)| {
        on_path(name).map(|program| Terminal {
            program,
            before: before.iter().map(|a| a.to_string()).collect(),
        })
    })
}

/// One run of DepotDownloader: the program, the folder it downloads into, and for a repair the
/// manifest paths to fetch again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub dd: PathBuf,
    pub folder: PathBuf,
    pub files: Option<Vec<String>>,
}

/// `path`, single-quoted for a POSIX shell.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// The command a player without the Steam app runs in their own terminal, on one line, their own
/// login left for them to type. It runs `dd`, the DepotDownloader Fenji fetched, by its path;
/// without one, the player gets the pinned release (the dialog says where) and runs it from its
/// folder, made executable first (#348). A repair names `list`, the failing files as Fenji listed
/// them ([`write_file_list`]), with `-validate`, as the terminal's script does (plan LP6).
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

/// Where a run is.
#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    /// Started; the player is signing in to Steam, or nothing has come yet.
    Waiting,
    Running {
        percent: f32,
        file: String,
    },
    Ended {
        code: i32,
    },
    /// The window closed before DepotDownloader ended, or the terminal never ran the script.
    Lost,
    /// The terminal failed before it ran the script: it opened nothing (#348).
    NotOpened,
}

/// A run of DepotDownloader in a terminal, followed through files in `run_dir`.
#[derive(Debug, Clone)]
pub struct DepotRun {
    pub run_dir: PathBuf,
    /// How long the terminal has to start the script before the run is [`RunState::Lost`].
    pub start_limit: Duration,
    started: Instant,
    /// The terminal's own process, once it has ended: whether it succeeded, and when. One that
    /// fails before the script starts opened nothing; one that succeeds may have handed the script
    /// to a server of its own, as `gnome-terminal` does, so only the start limit can tell; and one
    /// that fails may have handed it on first, so the script has [`HANDED_ON`] to show (#367).
    terminal: Arc<Mutex<Option<(bool, Instant)>>>,
}

/// How long after its terminal failed a run waits for the script before it says the terminal
/// opened nothing (#367).
const HANDED_ON: Duration = Duration::from_secs(2);

impl DepotRun {
    /// Writes the script (and a repair's file list) into a fresh `run_dir`, then opens the
    /// terminal on it, with `env` as it is outside an AppImage ([`appimage::outside`]). The
    /// terminal's own exit isn't waited on: many return at once.
    pub fn start(
        t: &Terminal,
        job: &Job,
        run_dir: &Path,
        env: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> std::io::Result<DepotRun> {
        let mut dirs = std::fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
        dirs.create(run_dir)?;
        let script = run_dir.join("depot.sh");
        std::fs::write(&script, SCRIPT)?;
        let mut cmd = std::process::Command::new(&t.program);
        cmd.args(&t.before)
            .arg("sh")
            .arg(&script)
            .arg(run_dir)
            .arg(&job.dd)
            .arg(&job.folder);
        if let Some(files) = &job.files {
            let list = run_dir.join("files.txt");
            write_file_list(&list, files)?;
            cmd.arg(list);
        }
        cmd.env_clear()
            .envs(appimage::outside(env))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn()?;
        // Reaped off this thread, so a terminal that lingers never leaves a zombie; its end is
        // kept for state().
        let terminal = Arc::new(Mutex::new(None));
        let ended = terminal.clone();
        std::thread::spawn(move || {
            let ok = child.wait().is_ok_and(|status| status.success());
            *ended
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((ok, Instant::now()));
        });
        Ok(DepotRun {
            run_dir: run_dir.to_path_buf(),
            start_limit: Duration::from_secs(30),
            started: Instant::now(),
            terminal,
        })
    }

    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.run_dir.join(name)).ok()
    }

    fn pgid(&self) -> Option<i32> {
        self.read("pgid")?.trim().parse().ok().filter(|&id| id > 0)
    }

    fn ended(&self) -> Option<RunState> {
        let code = self.read("status")?.trim().parse().unwrap_or(-1);
        Some(RunState::Ended { code })
    }

    /// Where the run is now (see [`RunState`]).
    pub fn state(&self) -> RunState {
        if let Some(ended) = self.ended() {
            return ended;
        }
        let Some(pgid) = self.pgid() else {
            let terminal = *self
                .terminal
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((false, at)) = terminal
                && at.elapsed() >= HANDED_ON
            {
                return RunState::NotOpened;
            }
            return if self.started.elapsed() > self.start_limit {
                RunState::Lost
            } else {
                RunState::Waiting
            };
        };
        if !group_alive(pgid) {
            // It may have ended between the two looks.
            return self.ended().unwrap_or(RunState::Lost);
        }
        self.read("progress")
            .and_then(|text| text.lines().rev().find_map(parse_progress))
            .map_or(RunState::Waiting, |(percent, file)| RunState::Running {
                percent,
                file,
            })
    }

    /// Ends the run: `SIGTERM` to the script's process group, DepotDownloader with it.
    pub fn cancel(&self) -> std::io::Result<()> {
        if let Some(pgid) = self.pgid() {
            signal_group(pgid)?;
        }
        Ok(())
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

#[cfg(unix)]
fn signal_group(pgid: i32) -> std::io::Result<()> {
    use rustix::process::{Pid, Signal, kill_process_group};
    let Some(pid) = Pid::from_raw(pgid) else {
        return Ok(());
    };
    match kill_process_group(pid, Signal::TERM) {
        Err(e) if e != rustix::io::Errno::SRCH => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(not(unix))]
fn signal_group(_: i32) -> std::io::Result<()> {
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

    /// Starts its command in a new session and returns at once, as `gnome-terminal` does.
    fn fake_terminal(dir: &Path) -> Terminal {
        Terminal {
            program: script(dir, "term", r#"setsid "$@" </dev/null >/dev/null 2>&1 &"#),
            before: vec![],
        }
    }

    fn wait_for(run: &DepotRun, done: impl Fn(&RunState) -> bool) -> RunState {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let s = run.state();
            if done(&s) || std::time::Instant::now() > until {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
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

    #[test]
    fn a_download_runs_in_its_terminal_then_reports_its_status() {
        let spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let dd = script(
            dir.path(),
            "dd",
            "echo 'QR here'; echo ' 50.00% a.bin'; echo '100.00% b.bin'; exit 0",
        );
        let job = Job {
            dd,
            folder: dir.path().join("game"),
            files: None,
        };
        let run = DepotRun::start(
            &fake_terminal(dir.path()),
            &job,
            &dir.path().join("run"),
            std::env::vars_os(),
        )
        .unwrap();
        drop(spawn);
        assert_eq!(
            wait_for(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 0 }
        );
        let progress = std::fs::read_to_string(run.run_dir.join("progress")).unwrap();
        assert!(progress.contains("100.00% b.bin") && !progress.contains("QR here"));
    }

    /// #348: a terminal opener that opens nothing (an `xdg-terminal-exec` with no terminal set up)
    /// fails at once, and the run says so then, not after waiting out its start limit.
    #[test]
    fn a_terminal_that_opens_nothing_is_known_at_once() {
        let _spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let term = Terminal {
            program: script(dir.path(), "term", "exit 1"),
            before: vec![],
        };
        let job = Job {
            dd: script(dir.path(), "dd", "exit 0"),
            folder: dir.path().join("game"),
            files: None,
        };
        let run = DepotRun::start(&term, &job, &dir.path().join("run"), []).unwrap();
        drop(_spawn);
        let started = std::time::Instant::now();
        assert_eq!(
            wait_for(&run, |s| *s != RunState::Waiting),
            RunState::NotOpened
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// #367: a terminal that hands the script to a server of its own and then fails is told apart
    /// from one that opened nothing: the run waits a moment for the script first.
    #[test]
    fn a_terminal_that_hands_the_script_on_then_fails_isnt_one_that_opened_nothing() {
        let _spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let term = Terminal {
            program: script(
                dir.path(),
                "term",
                r#"(sleep 0.5; setsid "$@" </dev/null >/dev/null 2>&1) & exit 1"#,
            ),
            before: vec![],
        };
        let job = Job {
            dd: script(dir.path(), "dd", "exit 0"),
            folder: dir.path().join("game"),
            files: None,
        };
        let run = DepotRun::start(&term, &job, &dir.path().join("run"), []).unwrap();
        drop(_spawn);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let end = loop {
            let s = run.state();
            assert_ne!(s, RunState::NotOpened, "the script was on its way");
            if matches!(s, RunState::Ended { .. } | RunState::Lost)
                || std::time::Instant::now() > until
            {
                break s;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(end, RunState::Ended { code: 0 });
    }

    /// #329: the terminal, and DepotDownloader in it, start outside the AppImage.
    #[test]
    fn the_terminal_gets_the_environment_outside_the_appimage() {
        let spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("seen");
        let term = Terminal {
            program: script(
                dir.path(),
                "term",
                &format!(
                    r#"env | sort > {}; setsid "$@" </dev/null >/dev/null 2>&1 &"#,
                    seen.display()
                ),
            ),
            before: vec![],
        };
        let job = Job {
            dd: script(dir.path(), "dd", "exit 0"),
            folder: dir.path().join("game"),
            files: None,
        };
        let env = [
            ("APPDIR", "/tmp/.mount_FenjiAb12"),
            ("PATH", "/tmp/.mount_FenjiAb12/usr/bin/:/usr/bin:/bin"),
            ("PYTHONHOME", "/tmp/.mount_FenjiAb12/usr/"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let run = DepotRun::start(&term, &job, &dir.path().join("run"), env).unwrap();
        drop(spawn);
        assert_eq!(
            wait_for(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 0 }
        );
        let text = std::fs::read_to_string(&seen).unwrap();
        let vars: Vec<&str> = text
            .lines()
            .filter(|l| !["PWD=", "SHLVL=", "_="].iter().any(|p| l.starts_with(p)))
            .collect();
        assert_eq!(vars, ["PATH=/usr/bin:/bin"]);
    }

    #[test]
    fn a_repair_passes_the_failing_files_and_validate_and_never_a_login() {
        let spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let dd = script(
            dir.path(),
            "dd",
            &format!("printf '%s\\n' \"$@\" > {}; exit 3", args.display()),
        );
        let files = vec![
            "bin64/a.dll".to_string(),
            "data/_index/sc_res.idx".to_string(),
        ];
        let job = Job {
            dd,
            folder: dir.path().join("game"),
            files: Some(files.clone()),
        };
        let run = DepotRun::start(
            &fake_terminal(dir.path()),
            &job,
            &dir.path().join("run"),
            std::env::vars_os(),
        )
        .unwrap();
        drop(spawn);
        assert_eq!(
            wait_for(&run, |s| matches!(s, RunState::Ended { .. })),
            RunState::Ended { code: 3 }
        );
        let got: Vec<String> = std::fs::read_to_string(&args)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        let list = got[got.iter().position(|a| a == "-filelist").unwrap() + 1].clone();
        assert_eq!(
            std::fs::read_to_string(list)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            files
        );
        assert!(got.contains(&"-validate".to_string()) && got.contains(&"-qr".to_string()));
        assert!(
            !got.iter()
                .any(|a| a == "-username" || a == "-password" || a == "-remember-password")
        );
    }

    #[test]
    fn a_window_closed_mid_download_ends_as_lost() {
        let spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let dd = script(dir.path(), "dd", "echo ' 10.00% a.bin'; sleep 30");
        let job = Job {
            dd,
            folder: dir.path().join("game"),
            files: None,
        };
        let run = DepotRun::start(
            &fake_terminal(dir.path()),
            &job,
            &dir.path().join("run"),
            std::env::vars_os(),
        )
        .unwrap();
        drop(spawn);
        assert!(matches!(
            wait_for(&run, |s| matches!(s, RunState::Running { .. })),
            RunState::Running { .. }
        ));
        let pgid: i32 = std::fs::read_to_string(run.run_dir.join("pgid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // As closing the window does:
        rustix::process::kill_process_group(
            rustix::process::Pid::from_raw(pgid).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        assert_eq!(wait_for(&run, |s| *s == RunState::Lost), RunState::Lost);
    }

    /// Review Focus 4 as a window really closes (#348): the terminal and its pty go, so the session
    /// is hung up (`SIGHUP`) rather than killed. Python's `pty` module stands in for the terminal.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_window_hung_up_mid_download_ends_as_lost() {
        let spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let holder = dir.path().join("holder");
        let term = Terminal {
            program: script(
                dir.path(),
                "term",
                &format!(
                    r#"python3 -I -c 'import os, pty, sys; open(sys.argv[1], "w").write(str(os.getpid())); pty.spawn(sys.argv[2:])' '{}' "$@" </dev/null >/dev/null 2>&1 &"#,
                    holder.display()
                ),
            ),
            before: vec![],
        };
        let dd = script(dir.path(), "dd", "echo ' 10.00% a.bin'; sleep 30");
        let job = Job {
            dd,
            folder: dir.path().join("game"),
            files: None,
        };
        let run =
            DepotRun::start(&term, &job, &dir.path().join("run"), std::env::vars_os()).unwrap();
        drop(spawn);
        assert!(
            matches!(
                wait_for(&run, |s| matches!(s, RunState::Running { .. })),
                RunState::Running { .. }
            ),
            "the fake terminal needs python3"
        );
        let pid: i32 = std::fs::read_to_string(&holder)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        assert_eq!(wait_for(&run, |s| *s == RunState::Lost), RunState::Lost);
    }

    #[test]
    fn cancel_ends_the_download_and_a_terminal_that_never_starts_is_lost() {
        let spawn = spawning();
        let dir = tempfile::tempdir().unwrap();
        let dd = script(dir.path(), "dd", "sleep 30");
        let job = Job {
            dd,
            folder: dir.path().join("game"),
            files: None,
        };
        let run = DepotRun::start(
            &fake_terminal(dir.path()),
            &job,
            &dir.path().join("run"),
            std::env::vars_os(),
        )
        .unwrap();
        wait_for(&run, |_| run.run_dir.join("pgid").exists());
        run.cancel().unwrap();
        assert!(matches!(
            wait_for(&run, |s| matches!(
                s,
                RunState::Lost | RunState::Ended { .. }
            )),
            RunState::Lost | RunState::Ended { .. }
        ));
        let dud = Terminal {
            program: script(dir.path(), "dud", "exit 0"),
            before: vec![],
        };
        let mut never =
            DepotRun::start(&dud, &job, &dir.path().join("run2"), std::env::vars_os()).unwrap();
        drop(spawn);
        never.start_limit = std::time::Duration::from_millis(300);
        assert_eq!(wait_for(&never, |s| *s == RunState::Lost), RunState::Lost);
    }

    #[test]
    fn terminals_are_found_in_order() {
        let path = |names: &'static [&'static str]| {
            move |n: &str| {
                names
                    .contains(&n)
                    .then(|| PathBuf::from(format!("/usr/bin/{n}")))
            }
        };
        let none = |_: &str| None;
        let t = find_terminal(&none, &path(&["xterm", "xdg-terminal-exec"])).unwrap();
        assert_eq!(
            (t.program, t.before),
            (PathBuf::from("/usr/bin/xdg-terminal-exec"), vec![])
        );
        let term = |v: &str| (v == "TERMINAL").then(|| OsString::from("foot"));
        let t = find_terminal(&term, &path(&["foot", "xterm"])).unwrap();
        assert_eq!(
            (t.program, t.before),
            (PathBuf::from("/usr/bin/foot"), vec!["-e".to_string()])
        );
        let t = find_terminal(&none, &path(&["konsole", "xterm"])).unwrap();
        assert_eq!(t.before, vec!["-e".to_string()]);
        assert!(find_terminal(&none, &path(&[])).is_none());
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
}
