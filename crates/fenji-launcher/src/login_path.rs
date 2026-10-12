//! How the client learns its gate, ticket and account (M3 spec D1–D3). Players use the launcher
//! path (`gj_uinfo`), the owner's decision on gate #114 (option a, 2026-10-13): it reads only inputs
//! the unmodified client already takes, and no platform service is involved
//! (docs/protocol/packets/lobby.md 12.6).
use std::net::SocketAddrV4;

use crate::api::GameGrant;
use crate::launch::LaunchSpec;

pub trait LoginPath {
    fn name(&self) -> &'static str;
    /// Adds what the client needs to log in to `gate` with `grant`.
    fn apply(&self, grant: &GameGrant, gate: SocketAddrV4, spec: &mut LaunchSpec);
}

/// The login paths there are, best first.
pub fn available() -> Vec<Box<dyn LoginPath>> {
    vec![Box::new(Uinfo)]
}

/// The launcher path (login.md 2.8–2.10): `gj_uinfo` (`<server id>:90:<server name>:0:<account>:
/// <user id>`), `gj_gates` and `gj_ticket`, all in the environment, where the client's setting
/// lookup finds them and the ticket stays out of the command line.
pub struct Uinfo;

impl LoginPath for Uinfo {
    fn name(&self) -> &'static str {
        "uinfo"
    }

    fn apply(&self, grant: &GameGrant, gate: SocketAddrV4, spec: &mut LaunchSpec) {
        let uinfo = format!(
            "{}:90:{}:0:{}:{}",
            grant.server_id, grant.server_name, grant.account, grant.user_id
        );
        spec.env.push(("gj_uinfo".into(), uinfo));
        spec.env.push(("gj_gates".into(), gate.to_string()));
        spec.env.push(("gj_ticket".into(), grant.ticket.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uinfo_puts_everything_in_the_environment() {
        let grant = GameGrant {
            ticket: "fenji1-ab".into(),
            expires_in_secs: 60,
            gate_host: "play.example.net".into(),
            gate_port: 7800,
            server_id: 1,
            server_name: "Fenji".into(),
            account: "tester".into(),
            user_id: 5,
        };
        let mut spec = LaunchSpec {
            program: "x".into(),
            args: vec![],
            env: vec![],
            cwd: "/".into(),
        };
        let paths = available();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].name(), "uinfo");
        paths[0].apply(&grant, "10.0.0.2:7800".parse().unwrap(), &mut spec);
        assert_eq!(
            spec.env,
            vec![
                ("gj_uinfo".to_string(), "1:90:Fenji:0:tester:5".to_string()),
                ("gj_gates".to_string(), "10.0.0.2:7800".to_string()),
                ("gj_ticket".to_string(), "fenji1-ab".to_string()),
            ]
        );
        assert!(
            spec.args.is_empty(),
            "the ticket stays out of the command line (S17)"
        );
    }
}
