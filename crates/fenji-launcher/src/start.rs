//! Starting the client, as a player's launcher does (M3 spec D14; plan Amendment B, the owner's
//! decision on gate #114). The order matters: the system is checked, the client folder is verified,
//! the Wine prefix and umu's runtime are made ready, and only then is a game token taken, since it
//! lives 60 s. The ticket reaches the client through its environment only, never its command line.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context, bail};

use crate::api::ApiClient;
use crate::launch::{LaunchSpec, Platform, base_spec, resolve_ipv4, umu_env};
use crate::login_path::available;
use crate::manifest::Manifest;
use crate::settings::Os;
use crate::verify::{Level, verify};

const NOT_HERE: &str = "Fenji can't start the client on this system yet";

/// Refuses every system but Linux, where the client runs through umu-run. Native Windows waits on
/// the owner's decision (plan Amendment B; ground rule 6), and macOS and the rest have no tested
/// way; all get the same neutral answer, `--print` included.
pub fn check_os(os: Os) -> anyhow::Result<()> {
    match os {
        Os::Linux => Ok(()),
        Os::MacOs | Os::Windows | Os::Other => bail!(NOT_HERE),
    }
}

/// `p`, or `p` under `cwd` when it's relative: the client runs from its `bin64/`, where a relative
/// path would point somewhere else.
pub fn anchor(p: &Path, cwd: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

/// What the command line may say about umu-run on Linux.
#[derive(Debug, Clone, Default)]
pub struct UmuOptions {
    /// umu-run: a path, or a name to look up on `PATH` (by default `umu-run`).
    pub umu_run: Option<PathBuf>,
    /// The launcher's Wine prefix (by default `prefix` in the launcher's data folder).
    pub prefix: Option<PathBuf>,
    /// A Proton build for `PROTONPATH` (by default umu-run's choice).
    pub proton: Option<PathBuf>,
}

/// The Linux platform from `opts`, every path absolute: relative ones are anchored at `cwd`, and a
/// bare umu-run name is looked up on `path` (the `PATH` variable).
pub fn linux_platform(
    opts: UmuOptions,
    cwd: &Path,
    path: Option<&OsStr>,
    data_dir: Option<PathBuf>,
) -> anyhow::Result<Platform> {
    let umu_run = opts.umu_run.unwrap_or_else(|| PathBuf::from("umu-run"));
    let umu_run = if umu_run.components().count() > 1 || umu_run.is_absolute() {
        anchor(&umu_run, cwd)
    } else {
        on_path(umu_run.as_os_str(), path).with_context(|| {
            format!(
                "{} isn't on PATH: install umu-launcher, or give its path with --umu-run",
                umu_run.display()
            )
        })?
    };
    let prefix = match opts.prefix {
        Some(p) => anchor(&p, cwd),
        None => data_dir
            .context("no data folder ($XDG_DATA_HOME or $HOME): give --prefix")?
            .join("prefix"),
    };
    Ok(Platform::Linux {
        umu_run,
        prefix,
        proton: opts.proton.map(|p| anchor(&p, cwd)),
    })
}

/// The first executable file named `name` in the directories of `path`. Empty and relative
/// entries are skipped: they would name whatever folder the launcher was started from.
fn on_path(name: &OsStr, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(name))
        .find(|p| executable(p))
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(p: &Path) -> bool {
    p.is_file()
}

/// Checks the client folder against `manifest` (sizes and the executables' hashes).
pub fn check_client(client: &Path, manifest: &Manifest) -> anyhow::Result<()> {
    let r = verify(client, manifest, Level::Quick)?;
    if !r.ok() {
        bail!(
            "the client folder isn't the pinned build: {} missing, {} of the wrong size, {} with the \
             wrong content (run `fenji-launch verify --client {}` for the list)",
            r.missing.len(),
            r.wrong_size.len(),
            r.wrong_hash.len(),
            client.display()
        );
    }
    Ok(())
}

/// The command that makes umu's runtime and the Wine prefix ready without starting the game:
/// `umu-run createprefix` (umu-launcher 1.4) in the launcher's prefix. umu hands Proton an empty
/// executable for it, so it exits 1 once the prefix is built; [`prefix_ready`] judges the result.
/// Nothing on Windows.
pub fn prepare_spec(platform: &Platform) -> Option<LaunchSpec> {
    match platform {
        Platform::Linux {
            umu_run,
            prefix,
            proton,
        } => Some(LaunchSpec {
            program: umu_run.clone(),
            args: vec!["createprefix".to_string()],
            env: umu_env(prefix, proton.as_deref()),
            cwd: prefix.clone(),
        }),
        Platform::Windows => None,
    }
}

/// Whether Proton has built the Wine prefix at `prefix`: its registry and its Windows folder.
pub fn prefix_ready(prefix: &Path) -> bool {
    prefix.join("system.reg").is_file() && prefix.join("drive_c/windows").is_dir()
}

/// Runs [`prepare_spec`] and judges it by the prefix, whatever umu-run's exit code.
fn prepare(platform: &Platform) -> anyhow::Result<()> {
    let (Some(spec), Platform::Linux { prefix, .. }) = (prepare_spec(platform), platform) else {
        return Ok(());
    };
    let status = run(&spec)?;
    if status.code().is_none() {
        bail!("`umu-run createprefix` was stopped ({status}), so the Wine prefix may not be ready");
    }
    if !prefix_ready(prefix) {
        bail!(
            "the Wine prefix in {} isn't ready; `umu-run createprefix`'s output above says why",
            prefix.display()
        );
    }
    Ok(())
}

/// Variables of this process that never reach umu-run, Proton or the client: the account's
/// password, which `fenji-launch` may read from its own environment. Inherited `gj_*` variables
/// never do either: only a fresh ticket's pairs may log the client in.
pub const SECRET_ENV: [&str; 1] = ["FENJI_PASSWORD"];

/// Compared without regard to case: Wine looks variables up that way.
fn passed_on(name: &OsStr) -> bool {
    let name = name.as_encoded_bytes();
    let secret = SECRET_ENV
        .iter()
        .any(|s| name.eq_ignore_ascii_case(s.as_bytes()));
    let login = name
        .get(..3)
        .is_some_and(|p| p.eq_ignore_ascii_case(b"gj_"));
    !secret && !login
}

/// The process for `spec`: its program, arguments and folder, and an environment of `parent`'s
/// variables less [`SECRET_ENV`] and `gj_*`, then the spec's own.
pub fn command(
    spec: &LaunchSpec,
    parent: impl IntoIterator<Item = (OsString, OsString)>,
) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .env_clear()
        .envs(parent.into_iter().filter(|(k, _)| passed_on(k)))
        .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    if !spec.cwd.as_os_str().is_empty() {
        cmd.current_dir(&spec.cwd);
    }
    cmd
}

/// Runs `spec` with this process's environment ([`command`]) and waits. Nothing of the environment
/// is printed.
pub fn run(spec: &LaunchSpec) -> anyhow::Result<ExitStatus> {
    if !spec.cwd.as_os_str().is_empty() {
        std::fs::create_dir_all(&spec.cwd)
            .with_context(|| format!("creating {}", spec.cwd.display()))?;
    }
    command(spec, std::env::vars_os())
        .status()
        .with_context(|| format!("running {}", spec.program.display()))
}

/// The exit status as a process exit code, as a shell reports it: the code itself, or 128 + N for
/// a signal N.
pub fn exit_byte(status: ExitStatus) -> u8 {
    if let Some(c) = status.code() {
        return u8::try_from(c).unwrap_or(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(s) = status.signal() {
            return u8::try_from(128 + s).unwrap_or(255);
        }
    }
    1
}

/// Who signs in, and where.
#[derive(Clone, Copy)]
pub struct Login<'a> {
    pub server: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

/// The login path's environment for one game token: `gj_uinfo`, `gj_gates` and `gj_ticket`. The
/// system is checked first ([`check_os`]), the gate before the password is sent (M3 spec §4),
/// `ready` runs before the token is taken, and the session ends whatever happens; `note` hears
/// about a failed sign-out.
pub fn take_ticket(
    os: Os,
    login: &Login,
    ready: impl FnOnce() -> anyhow::Result<()>,
    note: &mut dyn FnMut(&str),
) -> anyhow::Result<Vec<(String, String)>> {
    check_os(os)?;
    let paths = available();
    let path = paths.first().context("this build has no login path")?;
    let api = ApiClient::new(login.server)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let info = rt.block_on(api.server_info())?;
    let gate = resolve_ipv4(&info.gate_host, info.gate_port)?;
    ready()?;
    let session = rt.block_on(api.sign_in(login.username, login.password))?;
    let grant = rt.block_on(api.game_token(&session));
    // The ticket is all the client needs; the session ends here either way.
    if let Err(e) = rt.block_on(api.sign_out(session)) {
        note(&format!("signing out: {e}"));
    }
    let grant = grant?;
    if (grant.gate_host.as_str(), grant.gate_port) != (info.gate_host.as_str(), info.gate_port) {
        bail!("the server named one gate in its info and another in the game token");
    }
    Ok(path.env(&grant, gate))
}

/// Starts the game as a player's launcher does: checks the system ([`check_os`]), checks `client`
/// (anchored at `cwd`) against `manifest`, which `fenji-launch` always takes as the pinned build's
/// (ground rule 1), makes the prefix ready, takes a ticket ([`take_ticket`]) and runs the client
/// from its `bin64/` with the ticket in its environment. Returns how the client ended.
pub fn play(
    os: Os,
    client: &Path,
    cwd: &Path,
    manifest: &Manifest,
    platform: &Platform,
    login: &Login,
    note: &mut dyn FnMut(&str),
) -> anyhow::Result<ExitStatus> {
    check_os(os)?;
    if let Platform::Windows = platform {
        bail!(NOT_HERE);
    }
    let client = anchor(client, cwd);
    check_client(&client, manifest)?;
    let pairs = take_ticket(os, login, || prepare(platform), note)?;
    let mut spec = base_spec(&client, platform);
    spec.env.extend(pairs);
    note(&format!("starting the client for {}", login.username));
    run(&spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFUSAL: &str = "Fenji can't start the client on this system yet";

    use crate::test_support::spawning;

    #[test]
    fn only_linux_may_start_the_client_and_the_refusal_says_nothing_more() {
        assert!(check_os(Os::Linux).is_ok());
        for os in [Os::MacOs, Os::Windows, Os::Other] {
            assert_eq!(check_os(os).unwrap_err().to_string(), REFUSAL, "{os:?}");
        }
    }

    /// The library refuses before anything else, so a GUI over it can't skip the check: the
    /// server here would refuse a connection, so any other error means something was tried.
    #[test]
    fn the_library_refuses_other_systems_before_anything_is_sent() {
        let login = Login {
            server: "http://127.0.0.1:9",
            username: "tester",
            password: "a long password",
        };
        let platform = Platform::Linux {
            umu_run: "/nonexistent/umu-run".into(),
            prefix: "/nonexistent/prefix".into(),
            proton: None,
        };
        for os in [Os::MacOs, Os::Windows, Os::Other] {
            let e = take_ticket(os, &login, || Ok(()), &mut |_| {}).unwrap_err();
            assert_eq!(e.to_string(), REFUSAL, "take_ticket on {os:?}");
            let e = play(
                os,
                Path::new("/nonexistent/client"),
                Path::new("/"),
                &Manifest::pinned(),
                &platform,
                &login,
                &mut |_| {},
            )
            .unwrap_err();
            assert_eq!(e.to_string(), REFUSAL, "play on {os:?}");
        }
    }

    #[test]
    fn windows_is_refused_even_through_the_library() {
        let mut notes = Vec::new();
        let login = Login {
            server: "http://127.0.0.1:9",
            username: "tester",
            password: "a long password",
        };
        let e = play(
            Os::Linux,
            Path::new("/no/such/client"),
            Path::new("/"),
            &Manifest::pinned(),
            &Platform::Windows,
            &login,
            &mut |m| notes.push(m.to_string()),
        )
        .unwrap_err();
        assert_eq!(
            e.to_string(),
            REFUSAL,
            "refused before the folder is looked at"
        );
    }

    #[test]
    fn relative_paths_are_anchored_where_the_command_was_given() {
        let cwd = Path::new("/home/p/games");
        let opts = UmuOptions {
            umu_run: Some("tools/umu-run".into()),
            prefix: Some("pfx".into()),
            proton: Some("../GE-Proton11-7".into()),
        };
        let Platform::Linux {
            umu_run,
            prefix,
            proton,
        } = linux_platform(opts, cwd, None, None).unwrap()
        else {
            panic!("not Linux")
        };
        assert_eq!(umu_run, Path::new("/home/p/games/tools/umu-run"));
        assert_eq!(prefix, Path::new("/home/p/games/pfx"));
        assert_eq!(proton.unwrap(), Path::new("/home/p/games/../GE-Proton11-7"));
        assert_eq!(
            anchor(Path::new("/opt/umu-run"), cwd),
            Path::new("/opt/umu-run")
        );
        assert_eq!(anchor(Path::new("./umu-run"), cwd), cwd.join("./umu-run"));
    }

    #[cfg(unix)]
    fn script(p: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn umu_run_is_found_on_path_only_as_an_executable_in_an_absolute_folder() {
        let d = tempfile::tempdir().unwrap();
        let (plain, good) = (d.path().join("plain"), d.path().join("good"));
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::create_dir_all(&good).unwrap();
        script(&plain.join("umu-run"), 0o644);
        script(&good.join("umu-run"), 0o755);
        let path = std::env::join_paths([
            PathBuf::new(),
            PathBuf::from("relative"),
            plain.clone(),
            good.clone(),
        ])
        .unwrap();
        let opts = || UmuOptions {
            prefix: Some("/p".into()),
            ..UmuOptions::default()
        };
        let Platform::Linux { umu_run, .. } =
            linux_platform(opts(), Path::new("/"), Some(&path), None).unwrap()
        else {
            panic!("not Linux")
        };
        assert_eq!(umu_run, good.join("umu-run"));
        let e = linux_platform(opts(), Path::new("/"), Some(plain.as_os_str()), None)
            .unwrap_err()
            .to_string();
        assert!(e.contains("umu-launcher") && e.contains("--umu-run"), "{e}");
        let e = linux_platform(opts(), Path::new("/"), None, None)
            .unwrap_err()
            .to_string();
        assert!(e.contains("isn't on PATH"), "{e}");
        // A relative entry that does reach `good` from this process's folder is still skipped.
        let cwd = std::env::current_dir().unwrap();
        let up: PathBuf = cwd.components().skip(1).map(|_| "..").collect();
        let relative = up.join(good.strip_prefix("/").unwrap());
        assert!(relative.join("umu-run").is_file(), "{}", relative.display());
        let path = std::env::join_paths([relative, plain]).unwrap();
        assert!(linux_platform(opts(), Path::new("/"), Some(&path), None).is_err());
    }

    #[test]
    fn the_prefix_defaults_to_the_data_folder() {
        let opts = || UmuOptions {
            umu_run: Some("/usr/bin/umu-run".into()),
            ..UmuOptions::default()
        };
        let data = PathBuf::from("/home/p/.local/share/fenji-launcher");
        let Platform::Linux { prefix, .. } =
            linux_platform(opts(), Path::new("/"), None, Some(data.clone())).unwrap()
        else {
            panic!("not Linux")
        };
        assert_eq!(prefix, data.join("prefix"));
        let e = linux_platform(opts(), Path::new("/"), None, None)
            .unwrap_err()
            .to_string();
        assert!(e.contains("--prefix"), "{e}");
    }

    #[test]
    fn linux_prepares_with_createprefix_and_windows_needs_nothing() {
        let platform = Platform::Linux {
            umu_run: "/usr/bin/umu-run".into(),
            prefix: "/p/prefix".into(),
            proton: Some("/opt/proton".into()),
        };
        let spec = prepare_spec(&platform).unwrap();
        assert_eq!(spec.program, Path::new("/usr/bin/umu-run"));
        assert_eq!(spec.args, ["createprefix"]);
        assert!(
            spec.env
                .contains(&("WINEPREFIX".into(), "/p/prefix".into()))
        );
        assert!(spec.env.contains(&("GAMEID".into(), "umu-1418100".into())));
        assert!(
            spec.env
                .contains(&("PROTONPATH".into(), "/opt/proton".into()))
        );
        assert!(spec.env.iter().all(|(k, _)| !k.starts_with("gj_")));
        assert!(prepare_spec(&Platform::Windows).is_none());
    }

    #[test]
    fn a_prefix_is_ready_once_proton_has_built_it() {
        let d = tempfile::tempdir().unwrap();
        assert!(!prefix_ready(d.path()));
        std::fs::create_dir_all(d.path().join("drive_c/windows")).unwrap();
        assert!(!prefix_ready(d.path()), "no registry yet");
        std::fs::write(d.path().join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
        assert!(prefix_ready(d.path()));
    }

    /// umu 1.4 ends `createprefix` with exit 1 once the prefix is built; only the prefix and a
    /// signal decide, and the error never blames umu's normal status.
    #[cfg(unix)]
    #[test]
    fn preparing_is_judged_by_the_prefix_and_by_signals() {
        let _spawn = spawning();
        let d = tempfile::tempdir().unwrap();
        let umu = |name: &str, body: &str| {
            let p = d.path().join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let build =
            "mkdir -p \"$WINEPREFIX/drive_c/windows\" && echo x > \"$WINEPREFIX/system.reg\"";
        let platform = |umu_run: PathBuf, prefix: &str| Platform::Linux {
            umu_run,
            prefix: d.path().join(prefix),
            proton: None,
        };
        let built = umu("built", &format!("{build}\nexit 1"));
        let r = prepare(&platform(built, "a"));
        assert!(
            r.is_ok(),
            "exit 1 with a built prefix: {:#}",
            r.unwrap_err()
        );
        let empty = umu("empty", "exit 1");
        let e = format!("{:#}", prepare(&platform(empty, "b")).unwrap_err());
        assert!(e.contains("isn't ready"), "{e}");
        assert!(
            !e.contains("exit status"),
            "umu's normal exit isn't the cause: {e}"
        );
        let killed = umu("killed", &format!("{build}\nkill -9 $$"));
        let e = format!("{:#}", prepare(&platform(killed, "c")).unwrap_err());
        assert!(e.contains("signal"), "{e}");
    }

    #[test]
    fn a_folder_that_isnt_the_pinned_build_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let e = check_client(d.path(), &Manifest::pinned())
            .unwrap_err()
            .to_string();
        assert!(e.contains("210 missing"), "{e}");
    }

    /// Spawns the command for real: a variable the child inherits shows only in its own
    /// environment, never in `Command::get_envs`.
    #[cfg(unix)]
    #[test]
    fn the_child_gets_neither_the_password_nor_inherited_login_variables() {
        let _spawn = spawning();
        let spec = LaunchSpec {
            program: "/usr/bin/env".into(),
            args: vec![],
            env: vec![("gj_ticket".into(), "fenji1-new".into())],
            cwd: "/".into(),
        };
        let parent = [
            ("PATH", "/usr/bin"),
            ("FENJI_PASSWORD", "a long password"),
            ("gj_ticket", "fenji1-old"),
            // Wine looks names up whatever their case.
            ("GJ_GATES", "10.0.0.9:7800"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let out = command(&spec, parent).output().unwrap();
        let mut seen: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        seen.sort();
        assert_eq!(seen, ["PATH=/usr/bin", "gj_ticket=fenji1-new"]);
    }

    #[cfg(unix)]
    #[test]
    fn run_gives_the_program_its_environment_and_folder() {
        let _spawn = spawning();
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("seen");
        let spec = LaunchSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!(
                    "pwd > {0}; printf '%s' \"$gj_ticket\" >> {0}",
                    out.display()
                ),
            ],
            env: vec![("gj_ticket".into(), "fenji1-x".into())],
            cwd: d.path().join("bin64"),
        };
        assert!(run(&spec).unwrap().success());
        let seen = std::fs::read_to_string(out).unwrap();
        assert!(seen.ends_with("bin64\nfenji1-x"), "{seen}");
    }

    #[test]
    fn a_program_that_cant_start_is_named() {
        let _spawn = spawning();
        let d = tempfile::tempdir().unwrap();
        let spec = LaunchSpec {
            program: d.path().join("no-such-umu-run"),
            args: vec![],
            env: vec![],
            cwd: d.path().to_path_buf(),
        };
        let e = format!("{:#}", run(&spec).unwrap_err());
        assert!(e.contains("no-such-umu-run"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn exit_codes_and_signals_map_as_a_shell_reports_them() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(exit_byte(ExitStatus::from_raw(0)), 0);
        assert_eq!(exit_byte(ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(exit_byte(ExitStatus::from_raw(9)), 137, "SIGKILL");
        assert_eq!(exit_byte(ExitStatus::from_raw(15)), 143, "SIGTERM");
    }
}
