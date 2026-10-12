//! Starting the client, as a player's launcher does (M3 spec D14; plan Amendment B, the owner's
//! decision on gate #114). The order matters: the client folder is verified, the Wine prefix and
//! umu's runtime are made ready, and only then is a game token taken, since it lives 60 s. The
//! ticket reaches the client through its environment only, never its command line.
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::launch::{LaunchSpec, Platform, STEAM_APP_ID};
use crate::manifest::Manifest;
use crate::verify::{Level, verify};

/// What a start needs to know about this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    pub client: PathBuf,
    pub platform: Platform,
}

/// Checks the client folder against `manifest` (sizes and the executables' hashes).
pub fn check_client(client: &Path, manifest: &Manifest) -> anyhow::Result<()> {
    let r = verify(client, manifest, Level::Quick)?;
    if !r.ok() {
        anyhow::bail!(
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

/// The command that makes umu's runtime and the Wine prefix ready without starting the game: on
/// Linux, `umu-run` with Wine's own `wineboot` in the launcher's prefix. Nothing on Windows.
pub fn prepare_spec(platform: &Platform) -> Option<LaunchSpec> {
    match platform {
        Platform::Linux {
            umu_run,
            prefix,
            proton,
        } => {
            let mut env = vec![
                ("WINEPREFIX".to_string(), prefix.display().to_string()),
                ("GAMEID".to_string(), format!("umu-{STEAM_APP_ID}")),
            ];
            if let Some(p) = proton {
                env.push(("PROTONPATH".to_string(), p.display().to_string()));
            }
            Some(LaunchSpec {
                program: umu_run.clone(),
                args: vec!["wineboot".to_string(), "-u".to_string()],
                env,
                cwd: prefix.clone(),
            })
        }
        Platform::Windows => None,
    }
}

/// Variables of this process that must never reach umu-run, Proton or the client: the account's
/// password, which `fenji-launch` may read from its own environment.
pub const SECRET_ENV: [&str; 1] = ["FENJI_PASSWORD"];

/// Runs `spec` and waits: its arguments, its environment on top of this process's (without
/// [`SECRET_ENV`]), its working directory. Nothing of the environment is printed.
pub fn run(spec: &LaunchSpec) -> std::io::Result<ExitStatus> {
    if !spec.cwd.as_os_str().is_empty() {
        std::fs::create_dir_all(&spec.cwd)?;
    }
    let mut cmd = Command::new(&spec.program);
    for k in SECRET_ENV {
        cmd.env_remove(k);
    }
    cmd.args(&spec.args)
        .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(&spec.cwd)
        .status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_prepares_the_prefix_with_wineboot_and_windows_needs_nothing() {
        let platform = Platform::Linux {
            umu_run: "/usr/bin/umu-run".into(),
            prefix: "/p/prefix".into(),
            proton: Some("/opt/proton".into()),
        };
        let spec = prepare_spec(&platform).unwrap();
        assert_eq!(spec.program, Path::new("/usr/bin/umu-run"));
        assert_eq!(spec.args, ["wineboot", "-u"]);
        assert!(
            spec.env
                .contains(&("WINEPREFIX".into(), "/p/prefix".into()))
        );
        assert!(
            spec.env
                .contains(&("PROTONPATH".into(), "/opt/proton".into()))
        );
        assert!(spec.env.iter().all(|(k, _)| !k.starts_with("gj_")));
        assert!(prepare_spec(&Platform::Windows).is_none());
    }

    #[test]
    fn a_folder_that_isnt_the_pinned_build_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let e = check_client(d.path(), &Manifest::pinned())
            .unwrap_err()
            .to_string();
        assert!(e.contains("210 missing"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn run_gives_the_program_its_environment_and_folder() {
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
}
