//! The client's launch command as data (M3 spec D14): what to run, with which arguments,
//! environment and working directory. Nothing here starts a process.
use std::net::{SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::path::{Path, PathBuf};

/// Steam's app id for the game; `umu-run` uses it to pick its fixes (`GAMEID`).
pub const STEAM_APP_ID: u32 = 1418100;
/// The archive mount the client needs (login.md §1: `--fs=<dir>:<data index>:<asset index>`).
pub const FS_ARG: &str = "--fs=data:_index:_index";

/// Its `Debug` shows the environment's names, not their values: the login path puts the ticket
/// there.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

impl std::fmt::Debug for LaunchSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.env.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("LaunchSpec")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &names)
            .field("cwd", &self.cwd)
            .finish()
    }
}

/// Where the client runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    /// Through `umu-run`, in a Wine prefix of the launcher's own.
    Linux {
        umu_run: PathBuf,
        prefix: PathBuf,
        /// A Proton build for `PROTONPATH`; `umu-run` picks one when unset.
        proton: Option<PathBuf>,
    },
    Windows,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("{0} has no IPv4 address; the game client can only connect to IPv4 gates")]
    NoIpv4(String),
    #[error("can't look up {0}: {1}")]
    Lookup(String, String),
    #[error("{0} ({1}) can't be a gate: it isn't one machine's address")]
    Unusable(String, std::net::Ipv4Addr),
}

/// The command that starts the client in `client` (the folder holding `bin64/` and `data/`), run
/// from `bin64/`. It never carries an SDK flag (D1); the login path adds its environment.
pub fn base_spec(client: &Path, platform: &Platform) -> LaunchSpec {
    let bin64 = client.join("bin64");
    let exe = bin64.join("gujianol.exe");
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
            LaunchSpec {
                program: umu_run.clone(),
                args: vec![exe.display().to_string(), FS_ARG.to_string()],
                env,
                cwd: bin64,
            }
        }
        Platform::Windows => LaunchSpec {
            program: exe,
            args: vec![FS_ARG.to_string()],
            env: Vec::new(),
            cwd: bin64,
        },
    }
}

/// The gate as a dotted IPv4 address: the client parses it with `inet_addr` (transport.md 1.1).
pub fn resolve_ipv4(host: &str, port: u16) -> Result<SocketAddrV4, LaunchError> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| LaunchError::Lookup(host.to_string(), e.to_string()))?;
    let gate = addrs
        .filter_map(|a| match a {
            SocketAddr::V4(v4) => Some(v4),
            SocketAddr::V6(_) => None,
        })
        .next()
        .ok_or_else(|| LaunchError::NoIpv4(host.to_string()))?;
    let ip = *gate.ip();
    if ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
        return Err(LaunchError::Unusable(host.to_string(), ip));
    }
    Ok(gate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_never_shows_its_environment_values() {
        let spec = LaunchSpec {
            program: "x".into(),
            args: vec![],
            env: vec![("gj_ticket".into(), "fenji1-secret".into())],
            cwd: "/".into(),
        };
        let shown = format!("{spec:?}");
        assert!(
            shown.contains("gj_ticket") && !shown.contains("secret"),
            "{shown}"
        );
    }

    #[test]
    fn an_unusable_gate_address_is_refused() {
        for host in ["0.0.0.0", "255.255.255.255", "224.0.0.1"] {
            let e = resolve_ipv4(host, 7800).unwrap_err().to_string();
            assert!(e.contains("can't be a gate"), "{host}: {e}");
        }
    }

    #[test]
    fn linux_runs_umu_in_its_own_prefix_from_bin64() {
        let platform = Platform::Linux {
            umu_run: "/usr/bin/umu-run".into(),
            prefix: "/home/p/.local/share/fenji/prefix".into(),
            proton: None,
        };
        let spec = base_spec(Path::new("/games/solo"), &platform);
        assert_eq!(spec.program, Path::new("/usr/bin/umu-run"));
        assert_eq!(
            spec.args,
            vec![
                "/games/solo/bin64/gujianol.exe".to_string(),
                "--fs=data:_index:_index".to_string()
            ]
        );
        assert!(spec.env.contains(&(
            "WINEPREFIX".into(),
            "/home/p/.local/share/fenji/prefix".into()
        )));
        assert!(spec.env.contains(&("GAMEID".into(), "umu-1418100".into())));
        assert!(!spec.env.iter().any(|(k, _)| k == "PROTONPATH"));
        assert_eq!(spec.cwd, Path::new("/games/solo/bin64"));
        assert!(
            spec.args.iter().all(|a| !a.starts_with("--gf-sdk")),
            "never an SDK flag (D1)"
        );
        let with_proton = Platform::Linux {
            umu_run: "/usr/bin/umu-run".into(),
            prefix: "/p".into(),
            proton: Some("/opt/GE-Proton11-7".into()),
        };
        let spec = base_spec(Path::new("/games/solo"), &with_proton);
        assert!(
            spec.env
                .contains(&("PROTONPATH".into(), "/opt/GE-Proton11-7".into()))
        );
    }

    #[test]
    fn windows_runs_the_exe_directly() {
        let spec = base_spec(Path::new("C:/Games/SoLO"), &Platform::Windows);
        assert_eq!(spec.program, Path::new("C:/Games/SoLO/bin64/gujianol.exe"));
        assert_eq!(spec.args, vec!["--fs=data:_index:_index".to_string()]);
        assert_eq!(spec.cwd, Path::new("C:/Games/SoLO/bin64"));
        assert!(spec.env.is_empty());
    }

    #[test]
    fn gates_resolve_to_ipv4() {
        assert_eq!(
            resolve_ipv4("127.0.0.1", 7800).unwrap().to_string(),
            "127.0.0.1:7800"
        );
        assert_eq!(
            resolve_ipv4("localhost", 7800).unwrap().ip(),
            &std::net::Ipv4Addr::LOCALHOST
        );
    }

    #[test]
    fn gate_without_ipv4_is_a_clear_error() {
        let e = resolve_ipv4("::1", 7800).unwrap_err().to_string();
        assert!(e.contains("no IPv4 address"), "{e}");
        let e = resolve_ipv4("no-such-host.invalid", 7800)
            .unwrap_err()
            .to_string();
        assert!(e.contains("can't look up no-such-host.invalid"), "{e}");
    }
}
