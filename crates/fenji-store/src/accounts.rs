use sqlx::FromRow;

use crate::{Result, Store};

/// An account. Its `Debug` leaves out the password hash, so a logged account can't put it on disk.
#[derive(Clone, PartialEq, Eq, FromRow)]
pub struct Account {
    pub id: i64,
    pub username: String,
    /// The argon2id PHC string (ground rule 7).
    pub password_phc: String,
    pub created_at: i64,
    pub last_sign_in_at: Option<i64>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("password_phc", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("last_sign_in_at", &self.last_sign_in_at)
            .finish()
    }
}

/// An account as `account list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct AccountSummary {
    pub id: i64,
    pub username: String,
    pub created_at: i64,
    pub last_sign_in_at: Option<i64>,
    pub characters: i64,
}

impl Store {
    /// Usernames compare case-insensitively (the column is `NOCASE`).
    pub async fn account_by_username(&self, username: &str) -> Result<Option<Account>> {
        Ok(sqlx::query_as(
            "SELECT id, username, password_phc, created_at, last_sign_in_at FROM accounts \
             WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn account_by_id(&self, id: i64) -> Result<Option<Account>> {
        Ok(sqlx::query_as(
            "SELECT id, username, password_phc, created_at, last_sign_in_at FROM accounts \
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn record_sign_in(&self, id: i64, now: i64) -> Result<()> {
        sqlx::query("UPDATE accounts SET last_sign_in_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountSummary>> {
        Ok(sqlx::query_as(
            "SELECT a.id, a.username, a.created_at, a.last_sign_in_at, \
             (SELECT COUNT(*) FROM characters c WHERE c.account_id = a.id) AS characters \
             FROM accounts a ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Deletes the account. Its characters, sessions and game tokens go with it (foreign keys),
    /// and the invite it used stays spent (PRIVACY.md, "Deleting your data").
    pub async fn delete_account(&self, username: &str) -> Result<bool> {
        let r = sqlx::query("DELETE FROM accounts WHERE username = ?")
            .bind(username)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::Error;
    use crate::characters::NewCharacter;
    use crate::test_util::{NOW, account, h, store};

    #[tokio::test]
    async fn deleting_an_account_cascades_but_keeps_its_invite_spent() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let new = NewCharacter {
            name: "Lin".into(),
            vocation: 1,
            skeleton: 2,
            appearance: vec![1],
        };
        s.create_character(a, &new, 4, NOW).await.unwrap();
        s.create_session(&h(9), a, NOW, NOW + 10).await.unwrap();
        s.create_game_token(&h(10), a, NOW + 10).await.unwrap();
        assert!(s.delete_account("BOB").await.unwrap());
        assert!(!s.delete_account("bob").await.unwrap(), "already gone");
        assert!(s.account_by_username("bob").await.unwrap().is_none());
        assert!(s.list_characters(a).await.unwrap().is_empty());
        assert_eq!(s.session_account(&h(9), NOW).await.unwrap(), None);
        assert_eq!(s.consume_game_token(&h(10), NOW).await.unwrap(), None);
        assert!(matches!(
            s.redeem_invite(&h(1), "bob", "$p", NOW).await,
            Err(Error::InviteInvalid)
        ));
        let inv = &s.list_invites().await.unwrap()[0];
        assert!(inv.redeemed_at.is_some() && inv.redeemed_by.is_none());
    }

    #[tokio::test]
    async fn sign_ins_are_recorded_and_accounts_listed() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        s.record_sign_in(a, NOW + 5).await.unwrap();
        let list = s.list_accounts().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (
                list[0].username.as_str(),
                list[0].last_sign_in_at,
                list[0].characters
            ),
            ("bob", Some(NOW + 5), 0)
        );
        assert_eq!(s.account_by_username("BOB").await.unwrap().unwrap().id, a);
        let acc = s.account_by_id(a).await.unwrap().unwrap();
        assert_eq!((acc.password_phc.as_str(), acc.created_at), ("$phc", NOW));
        assert!(
            !format!("{acc:?}").contains("$phc"),
            "Debug leaves out the hash"
        );
    }
}
