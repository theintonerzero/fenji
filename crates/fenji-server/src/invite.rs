//! Making an invite (M3 spec D13; M4 spec §4.3): one place for `fenji-server invite create`, the
//! admins' `!invite` and the host console's `invite`. Its links (M4 hosting spec §3.10, §3.11 and
//! §8, HA4) start where players reach the account API, and carry the server's pin when it serves
//! TLS with its own key.
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use fenji_auth::invite::InviteCode;
use fenji_store::Store;

use crate::commands::check_label;
use crate::config::{Config, Tls};

/// Where an invite link starts: the account API's origin as players reach it,
/// `scheme://host[:port]`, and the pin of the server's own key when it serves TLS with one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteBase {
    pub origin: String,
    pub pin: Option<String>,
}

impl InviteBase {
    /// The link for invite `code`: `<origin>/#invite=<code>`, then `&pin=<pin>` with a pin. Both
    /// are in the fragment, which never reaches a server.
    pub fn invite_link(&self, code: &str) -> String {
        match &self.pin {
            Some(pin) => format!("{}/#invite={code}&pin={pin}", self.origin),
            None => format!("{}/#invite={code}", self.origin),
        }
    }
}

/// The link to the server itself, without an invite, for players who already have an account (M4
/// hosting spec §3.10): `<origin>/#pin=<pin>` with a pin, and the origin alone without one, as
/// behind Caddy. Launchers add a server by either.
pub fn server_link(base: &InviteBase) -> String {
    match &base.pin {
        Some(pin) => format!("{}/#pin={pin}", base.origin),
        None => base.origin.clone(),
    }
}

/// What a server with no link beyond this computer needs (M4 hosting spec §3.11): `run` warns with
/// it at start, and `invite` and `link` say it. It points at this version's guide on the web, as
/// the archive holds no `docs/hosting.md` (the hosting plan's final review B, Minor 7).
pub fn no_links() -> String {
    format!(
        "invite and server links need [server] public_api or tls = \"self\": see {}",
        crate::config::guide()
    )
}

/// The label of the link for this computer, whose API is on loopback.
pub const THIS_COMPUTER: &str = "this computer";

/// The invite links' bases, each labelled with where it reaches the account API, `api_port` being
/// the port it listens on and `pin` the server's own key's (Amendment A, HA4):
/// - with `public_api` or `public_host` set, the one [`Config::invite_base`] gives;
/// - with neither, one for each non-loopback IPv4 address of the machine's interfaces, labelled
///   with the interface's name, then loopback's, labelled [`THIS_COMPUTER`], last.
///
/// Links leave this computer only over TLS, as launchers take plain http from loopback alone (M3
/// spec D12), and never without the pin when the server has its own key. Reading the interfaces
/// sends nothing over the network.
pub fn bases(cfg: &Config, pin: Option<&str>, api_port: u16) -> Vec<(String, InviteBase)> {
    bases_among(cfg, pin, api_port, &interfaces())
}

/// The machine's IPv4 addresses, each with its interface's name. None when they can't be read:
/// links for this computer still come.
fn interfaces() -> Vec<(String, Ipv4Addr)> {
    match if_addrs::get_if_addrs() {
        Ok(list) => list
            .into_iter()
            .filter_map(|i| match i.ip() {
                IpAddr::V4(ip) => Some((i.name, ip)),
                IpAddr::V6(_) => None,
            })
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "can't list this machine's addresses for invite links");
            Vec::new()
        }
    }
}

/// [`bases`], with the machine's `interfaces` given.
pub(crate) fn bases_among(
    cfg: &Config,
    pin: Option<&str>,
    api_port: u16,
    interfaces: &[(String, Ipv4Addr)],
) -> Vec<(String, InviteBase)> {
    let s = &cfg.server;
    if s.public_api.is_some() || s.public_host.is_some() {
        return cfg
            .labelled_invite_base(pin, api_port)
            .map(|(label, base)| (label.to_string(), base))
            .into_iter()
            .collect();
    }
    let own_key = s.tls == Some(Tls::OwnKey);
    if own_key && pin.is_none() {
        return Vec::new();
    }
    let base = |ip: IpAddr| {
        let scheme = if own_key { "https" } else { "http" };
        InviteBase {
            origin: format!("{scheme}://{}", SocketAddr::new(ip, api_port)),
            pin: pin.filter(|_| own_key).map(str::to_owned),
        }
    };
    let ip = s.api_bind.ip().to_canonical();
    let mut out = Vec::new();
    if ip.is_unspecified() {
        // Every interface, IPv4 ones through an IPv6 wildcard too (dual-stack, as systems make
        // them by default). Plain http leaves this computer for no one.
        if own_key {
            for (name, v4) in interfaces.iter().filter(|(_, v4)| !v4.is_loopback()) {
                out.push((name.clone(), base(IpAddr::V4(*v4))));
            }
        }
        out.push((THIS_COMPUTER.into(), base(IpAddr::V4(Ipv4Addr::LOCALHOST))));
    } else if ip.is_loopback() {
        out.push((THIS_COMPUTER.into(), base(ip)));
    } else if let (IpAddr::V4(v4), true) = (ip, own_key) {
        let label = interfaces
            .iter()
            .find(|(_, a)| *a == v4)
            .map_or_else(|| v4.to_string(), |(name, _)| name.clone());
        out.push((label, base(ip)));
    }
    out
}

/// A new invite: its code, and the link players open when the server has an address for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInvite {
    pub code: String,
    pub link: Option<String>,
}

/// Makes an invite with `label`, expiring at `expires_at` if given. Its link starts with `base`
/// (`Config::invite_base`); callers with several bases make each one's with
/// [`InviteBase::invite_link`]. A label `!invite` would refuse is refused here too, with its
/// [`CommandError`](crate::commands::CommandError), and nothing is stored: `invite list` shows
/// labels in a host's terminal.
pub async fn create(
    store: &Store,
    base: Option<&InviteBase>,
    label: Option<&str>,
    now: i64,
    expires_at: Option<i64>,
) -> anyhow::Result<NewInvite> {
    if let Some(label) = label {
        check_label(label)?;
    }
    let code = InviteCode::generate()?;
    store
        .create_invite(&code.hash(), label, now, expires_at)
        .await?;
    let code = code.display();
    let link = base.map(|base| base.invite_link(&code));
    Ok(NewInvite { code, link })
}

/// The answer `!invite` whispers back.
impl fmt::Display for NewInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.link {
            Some(link) => write!(f, "invite link: {link}"),
            None => write!(
                f,
                "invite code: {} (no link: this server has no one address for links; the host's \
                 `fenji-server invite create` gives them)",
                self.code
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::path::Path;

    use super::*;
    use crate::commands::{CommandError, INVITE};

    const PIN: &str = "q1o9C1M6ufA5UGrUkSYhW-AXXa3S7jdJ9Fxlp0f2cBo";

    fn base(origin: &str, pin: Option<&str>) -> InviteBase {
        InviteBase {
            origin: origin.into(),
            pin: pin.map(Into::into),
        }
    }

    fn labelled(label: &str, origin: &str, pin: Option<&str>) -> (String, InviteBase) {
        (label.into(), base(origin, pin))
    }

    fn cfg(server: &str) -> Config {
        Config::parse(&format!("[server]\nname = \"F\"\n{server}"), Path::new("/")).unwrap()
    }

    /// A LAN card, a VPN and loopback, as `if-addrs` lists them.
    fn interfaces() -> Vec<(String, Ipv4Addr)> {
        vec![
            ("eth0".into(), Ipv4Addr::new(192, 168, 1, 10)),
            ("lo".into(), Ipv4Addr::LOCALHOST),
            ("tailscale0".into(), Ipv4Addr::new(100, 64, 0, 7)),
        ]
    }

    #[test]
    fn links_carry_the_code_and_the_pin_in_the_fragment() {
        let pinned = base("https://203.0.113.5:7801", Some(PIN));
        assert_eq!(
            pinned.invite_link("ABCD-EFGH-JKMN-PQRS"),
            format!("https://203.0.113.5:7801/#invite=ABCD-EFGH-JKMN-PQRS&pin={PIN}")
        );
        assert_eq!(
            server_link(&pinned),
            format!("https://203.0.113.5:7801/#pin={PIN}")
        );
        let plain = base("http://127.0.0.1:7801", None);
        assert_eq!(
            plain.invite_link("ABCD-EFGH-JKMN-PQRS"),
            "http://127.0.0.1:7801/#invite=ABCD-EFGH-JKMN-PQRS"
        );
        // Without a pin, as behind Caddy, the server's link is its address alone (the hosting
        // plan's final review, Important 2).
        assert_eq!(server_link(&plain), "http://127.0.0.1:7801");
        let named = base("https://play.example.net", None);
        assert_eq!(server_link(&named), "https://play.example.net");
    }

    #[test]
    fn with_no_one_address_set_every_address_of_the_machine_gets_a_link() {
        // Amendment A, HA4: each non-loopback IPv4 address, labelled with its interface, then this
        // computer's, last.
        let own = cfg("tls = \"self\"\napi_bind = \"0.0.0.0:7801\"\n");
        assert_eq!(
            bases_among(&own, Some(PIN), 7801, &interfaces()),
            [
                labelled("eth0", "https://192.168.1.10:7801", Some(PIN)),
                labelled("tailscale0", "https://100.64.0.7:7801", Some(PIN)),
                labelled("this computer", "https://127.0.0.1:7801", Some(PIN)),
            ]
        );
        // The port is the one the API listens on, which the caller knows.
        let on_9000 = bases_among(&own, Some(PIN), 9000, &interfaces());
        assert!(
            on_9000.iter().all(|(_, b)| b.origin.ends_with(":9000")),
            "{on_9000:?}"
        );
        // An IPv6 wildcard takes IPv4 too.
        let dual = cfg("tls = \"self\"\napi_bind = \"[::]:7801\"\n");
        assert_eq!(bases_among(&dual, Some(PIN), 7801, &interfaces()).len(), 3);
        // No machine addresses: this computer still has one.
        assert_eq!(
            bases_among(&own, Some(PIN), 7801, &[]),
            [labelled(
                "this computer",
                "https://127.0.0.1:7801",
                Some(PIN)
            )]
        );
        // No pin, no link: a launcher couldn't trust the key.
        assert_eq!(bases_among(&own, None, 7801, &interfaces()), []);
    }

    #[test]
    fn an_api_on_one_address_gets_that_link_alone() {
        let vpn = cfg("tls = \"self\"\napi_bind = \"100.64.0.7:7801\"\n");
        assert_eq!(
            bases_among(&vpn, Some(PIN), 7801, &interfaces()),
            [labelled("tailscale0", "https://100.64.0.7:7801", Some(PIN))]
        );
        let local = cfg("tls = \"self\"\napi_bind = \"127.0.0.1:7801\"\n");
        assert_eq!(
            bases_among(&local, Some(PIN), 7801, &interfaces()),
            [labelled(
                "this computer",
                "https://127.0.0.1:7801",
                Some(PIN)
            )]
        );
        // An address no interface has now is named as it is.
        let gone = cfg("tls = \"self\"\napi_bind = \"10.9.8.7:7801\"\n");
        assert_eq!(
            bases_among(&gone, Some(PIN), 7801, &interfaces()),
            [labelled("10.9.8.7", "https://10.9.8.7:7801", Some(PIN))]
        );
    }

    #[test]
    fn what_links_need_points_at_this_versions_guide() {
        // The archive holds no docs/hosting.md (the hosting plan's final review B, Minor 7).
        assert_eq!(
            no_links(),
            format!(
                "invite and server links need [server] public_api or tls = \"self\": see {}",
                crate::config::guide()
            )
        );
    }

    #[test]
    fn without_tls_only_this_computer_gets_a_link() {
        // Launchers take plain http from this computer only (M3 spec D12).
        let plain = cfg("api_bind = \"0.0.0.0:7801\"\n");
        assert_eq!(
            bases_among(&plain, None, 7801, &interfaces()),
            [labelled("this computer", "http://127.0.0.1:7801", None)]
        );
        let lan = cfg("api_bind = \"192.168.1.10:7801\"\n");
        assert_eq!(bases_among(&lan, None, 7801, &interfaces()), []);
        let local = cfg("");
        assert_eq!(
            bases_among(&local, None, 7801, &interfaces()),
            [labelled("this computer", "http://127.0.0.1:7801", None)]
        );
    }

    #[test]
    fn with_public_api_or_public_host_there_is_one_link() {
        let named = cfg("public_api = \"https://play.example.net\"\napi_bind = \"0.0.0.0:7801\"\n");
        assert_eq!(
            bases_among(&named, None, 7801, &interfaces()),
            [labelled("public_api", "https://play.example.net", None)]
        );
        let host =
            cfg("tls = \"self\"\npublic_host = \"203.0.113.5\"\napi_bind = \"0.0.0.0:7801\"\n");
        assert_eq!(
            bases_among(&host, Some(PIN), 7801, &interfaces()),
            [labelled(
                "public_host",
                "https://203.0.113.5:7801",
                Some(PIN)
            )]
        );
        let local_host = cfg("public_host = \"127.0.0.1\"\n");
        assert_eq!(
            bases_among(&local_host, None, 7801, &interfaces()),
            [labelled("this computer", "http://127.0.0.1:7801", None)]
        );
        // #174: a public gate, a loopback API and neither public_api nor tls: no usable link.
        let proxied = cfg("public_host = \"203.0.113.5\"\n");
        assert_eq!(bases_among(&proxied, None, 7801, &interfaces()), []);
    }

    #[tokio::test]
    async fn a_pinned_invite_link_carries_the_code_then_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("f.db")).await.unwrap();
        let pinned = base("https://127.0.0.1:7801", Some(PIN));
        let made = create(&store, Some(&pinned), None, 1, None).await.unwrap();
        let link = format!("https://127.0.0.1:7801/#invite={}&pin={PIN}", made.code);
        assert_eq!(made.link.as_deref(), Some(link.as_str()));
        assert_eq!(made.to_string(), format!("invite link: {link}"));
    }

    #[tokio::test]
    async fn an_invite_is_stored_open_and_its_link_carries_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("f.db")).await.unwrap();
        let made = create(
            &store,
            Some(&base("https://play.example.net", None)),
            Some("for Bob"),
            1,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            made.link.as_deref(),
            Some(format!("https://play.example.net/#invite={}", made.code).as_str())
        );
        assert_eq!(
            made.to_string(),
            format!(
                "invite link: https://play.example.net/#invite={}",
                made.code
            )
        );
        let code = InviteCode::parse(&made.code).unwrap();
        assert!(store.invite_is_open(&code.hash(), 2).await.unwrap());
        assert_eq!(
            store.list_invites().await.unwrap()[0].label.as_deref(),
            Some("for Bob")
        );
        let bare = create(&store, None, None, 1, None).await.unwrap();
        assert_eq!(bare.link, None);
        assert!(
            bare.to_string()
                .starts_with(&format!("invite code: {} (", bare.code))
        );
    }

    #[tokio::test]
    async fn a_label_not_plain_or_over_64_characters_is_refused_and_nothing_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("f.db")).await.unwrap();
        let long = "x".repeat(65);
        for (label, why) in [
            ("\u{1b}]0;owned\u{7}", CommandError::NotPlain),
            ("for\u{202E}boB", CommandError::NotPlain),
            (long.as_str(), CommandError::Usage(INVITE)),
        ] {
            let e = create(&store, None, Some(label), 1, None)
                .await
                .unwrap_err();
            assert_eq!(e.downcast_ref::<CommandError>(), Some(&why), "{label:?}");
        }
        assert!(store.list_invites().await.unwrap().is_empty());
        // 64 characters, in 128 bytes.
        let longest = "é".repeat(64);
        create(&store, None, Some(&longest), 1, None).await.unwrap();
        assert_eq!(
            store.list_invites().await.unwrap()[0].label.as_deref(),
            Some(longest.as_str())
        );
    }
}
