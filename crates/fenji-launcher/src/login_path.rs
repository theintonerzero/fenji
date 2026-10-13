//! How the client learns its gate, ticket and account (M3 spec D1–D3). Players use the launcher
//! path (`gj_uinfo`), the owner's decision on gate #114 (option a, 2026-10-13): it reads only inputs
//! the unmodified client already takes, and no platform service is involved
//! (docs/protocol/packets/lobby.md 12.6).
use std::net::SocketAddrV4;

use crate::api::GameGrant;

pub trait LoginPath {
    fn name(&self) -> &'static str;
    /// What the client needs to log in to `gate` with `grant`, as environment variables: a login
    /// path never adds arguments, so the ticket can't reach a command line.
    fn env(&self, grant: &GameGrant, gate: SocketAddrV4) -> Vec<(String, String)>;
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

    fn env(&self, grant: &GameGrant, gate: SocketAddrV4) -> Vec<(String, String)> {
        let uinfo = format!(
            "{}:90:{}:0:{}:{}",
            grant.server_id, grant.server_name, grant.account, grant.user_id
        );
        vec![
            ("gj_uinfo".into(), uinfo),
            ("gj_gates".into(), gate.to_string()),
            ("gj_ticket".into(), grant.ticket.clone()),
        ]
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
        let paths = available();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].name(), "uinfo");
        let env = paths[0].env(&grant, "10.0.0.2:7800".parse().unwrap());
        assert_eq!(
            env,
            vec![
                ("gj_uinfo".to_string(), "1:90:Fenji:0:tester:5".to_string()),
                ("gj_gates".to_string(), "10.0.0.2:7800".to_string()),
                ("gj_ticket".to_string(), "fenji1-ab".to_string()),
            ]
        );
    }
}
