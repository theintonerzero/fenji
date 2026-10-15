//! A server's state as the launcher sees it (launcher spec §3.3, L10; plan LP9): online, in
//! maintenance with the host's message, offline, its key changed, its own key without a pin, or
//! one this launcher can't play on.

use std::time::{Duration, Instant};

use crate::api::{ApiClient, ApiError, ServerInfo, ServerState};

/// How long a probe waits for an answer before it calls the server offline.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The newest account API this launcher speaks.
pub const API_VERSION: u32 = 1;

/// A server's state, as the launcher shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Online,
    /// The host's message (empty when they gave none), and when they expect to be back.
    Maintenance {
        message: String,
        until: Option<i64>,
    },
    /// No answer, or none that made sense.
    Offline,
    /// A pinned address reached a server with another key.
    KeyChanged,
    /// An address without a pin reached a server with its own key.
    OwnKey,
    Incompatible(Incompatible),
}

/// Why this launcher can't play on a server that answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incompatible {
    /// It expects another build of the game than the one this launcher checks.
    Build,
    /// It runs a newer Fenji: a newer API, or a state this launcher doesn't know (plan LP9).
    Newer,
}

/// One probe's result: the state, what the server said, and how long it took to say it.
#[derive(Debug, Clone)]
pub struct Probe {
    pub status: Status,
    pub info: Option<ServerInfo>,
    pub ping_ms: Option<u32>,
}

/// What an answer (or its failure) means. The server's info comes back with every answer.
pub fn classify(
    answer: Result<ServerInfo, ApiError>,
    pinned_build: &str,
) -> (Status, Option<ServerInfo>) {
    match answer {
        Ok(info) => {
            let status = if info.pinned_build != pinned_build {
                Status::Incompatible(Incompatible::Build)
            } else if info.api_version > API_VERSION {
                Status::Incompatible(Incompatible::Newer)
            } else {
                match info.state {
                    ServerState::Online => Status::Online,
                    ServerState::Maintenance => {
                        let (message, until) = info
                            .maintenance
                            .as_ref()
                            .map_or((String::new(), None), |m| (m.message.clone(), m.until));
                        Status::Maintenance { message, until }
                    }
                    ServerState::Unknown => Status::Incompatible(Incompatible::Newer),
                }
            };
            (status, Some(info))
        }
        Err(ApiError::KeyChanged) => (Status::KeyChanged, None),
        Err(ApiError::OwnKey) => (Status::OwnKey, None),
        Err(_) => (Status::Offline, None),
    }
}

/// Asks the server for its facts, waiting at most [`PROBE_TIMEOUT`].
pub async fn probe(api: &ApiClient, pinned_build: &str) -> Probe {
    probe_within(api, pinned_build, PROBE_TIMEOUT).await
}

/// [`probe`] with its own time limit. The ping is the request's round trip, in whole
/// milliseconds and at least 1; a server kept by one `ApiClient` keeps its connection open, so
/// later pings measure the network rather than a handshake.
pub async fn probe_within(api: &ApiClient, pinned_build: &str, limit: Duration) -> Probe {
    let started = Instant::now();
    match tokio::time::timeout(limit, api.server_info()).await {
        Ok(answer) => {
            let answered = answer.is_ok();
            let ping = started.elapsed().as_millis().clamp(1, u128::from(u32::MAX)) as u32;
            let (status, info) = classify(answer, pinned_build);
            Probe {
                status,
                info,
                ping_ms: answered.then_some(ping),
            }
        }
        Err(_) => Probe {
            status: Status::Offline,
            info: None,
            ping_ms: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn says(v: serde_json::Value) -> Status {
        classify(Ok(serde_json::from_value(v).unwrap()), "10015727").0
    }

    #[test]
    fn what_a_server_says_becomes_its_status() {
        let base = json!({"name": "Moonward", "version": "0.5.0", "pinned_build": "10015727",
            "gate_host": "203.0.113.5", "gate_port": 7800, "server_id": 1, "api_version": 1});
        assert_eq!(says(base.clone()), Status::Online);
        let mut m = base.clone();
        m["state"] = json!("maintenance");
        m["maintenance"] = json!({"message": "Back soon", "since": 1, "until": 2});
        assert_eq!(
            says(m),
            Status::Maintenance {
                message: "Back soon".into(),
                until: Some(2)
            }
        );
        let mut b = base.clone();
        b["pinned_build"] = json!("99999999");
        assert_eq!(says(b), Status::Incompatible(Incompatible::Build));
        let mut n = base.clone();
        n["api_version"] = json!(2);
        assert_eq!(says(n), Status::Incompatible(Incompatible::Newer));
        let mut u = base;
        u["state"] = json!("sleeping");
        assert_eq!(says(u), Status::Incompatible(Incompatible::Newer));
        assert_eq!(
            classify(Err(ApiError::KeyChanged), "10015727").0,
            Status::KeyChanged
        );
        assert_eq!(
            classify(Err(ApiError::OwnKey), "10015727").0,
            Status::OwnKey
        );
        assert_eq!(
            classify(Err(ApiError::Http("refused".into())), "10015727"),
            (Status::Offline, None)
        );
    }
}
