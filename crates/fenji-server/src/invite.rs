//! Making an invite (M3 spec D13; M4 spec §4.3): one place for `fenji-server invite create` and
//! the admins' `!invite`, and later the host console's `invite`.
use std::fmt;

use fenji_auth::invite::InviteCode;
use fenji_store::Store;

/// A new invite: its code, and the link players open when the server has an address for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInvite {
    pub code: String,
    pub link: Option<String>,
}

/// Makes an invite with `label`, expiring at `expires_at` if given. Links start with `base`
/// (`Config::invite_base`).
pub async fn create(
    store: &Store,
    base: Option<&str>,
    label: Option<&str>,
    now: i64,
    expires_at: Option<i64>,
) -> anyhow::Result<NewInvite> {
    let code = InviteCode::generate()?;
    store
        .create_invite(&code.hash(), label, now, expires_at)
        .await?;
    let code = code.display();
    let link = base.map(|base| format!("{base}/#invite={code}"));
    Ok(NewInvite { code, link })
}

/// The answer `!invite` whispers back.
impl fmt::Display for NewInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.link {
            Some(link) => write!(f, "invite link: {link}"),
            None => write!(
                f,
                "invite code: {} (no link: set [server] public_api to the address players reach \
                 the account API at)",
                self.code
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_invite_is_stored_open_and_its_link_carries_the_code() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("f.db")).await.unwrap();
        let made = create(
            &store,
            Some("https://play.example.net"),
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
}
