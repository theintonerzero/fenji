use sqlx::FromRow;

use crate::error::is_unique;
use crate::{Error, Result, Store};

/// An invite as `invite list` shows it: never its code, which only its creator saw.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct InviteRow {
    pub id: i64,
    pub label: Option<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
    /// The account's username, or `None` if unused or the account was deleted.
    pub redeemed_by: Option<String>,
    pub redeemed_at: Option<i64>,
}

impl Store {
    /// Returns the invite's id, which `revoke_invite` takes.
    pub async fn create_invite(
        &self,
        code_hash: &[u8; 32],
        label: Option<&str>,
        now: i64,
        expires_at: Option<i64>,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO invites (code_hash, label, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&code_hash[..])
        .bind(label)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn list_invites(&self) -> Result<Vec<InviteRow>> {
        Ok(sqlx::query_as(
            "SELECT i.id, i.label, i.created_at, i.expires_at, i.revoked_at, \
             a.username AS redeemed_by, i.redeemed_at \
             FROM invites i LEFT JOIN accounts a ON a.id = i.redeemed_by ORDER BY i.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Revokes an invite nobody has used; false if it was used or revoked already, or doesn't exist.
    pub async fn revoke_invite(&self, id: i64, now: i64) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE invites SET revoked_at = ? \
             WHERE id = ? AND revoked_at IS NULL AND redeemed_at IS NULL",
        )
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Whether an invite could be redeemed now: known, unspent, unrevoked and unexpired. It spends
    /// nothing; `redeem_invite` checks again in its transaction. The account API asks first, so a
    /// fake code costs no password hashing (M3 plan, Amendment A).
    pub async fn invite_is_open(&self, code_hash: &[u8; 32], now: i64) -> Result<bool> {
        let open: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM invites WHERE code_hash = ? AND redeemed_at IS NULL \
             AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(&code_hash[..])
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(open.is_some())
    }

    /// The id and label of the invite the account `account_id` was made with, if it was, for the
    /// server's line on a new account.
    pub async fn invite_of_account(
        &self,
        account_id: i64,
    ) -> Result<Option<(i64, Option<String>)>> {
        Ok(
            sqlx::query_as("SELECT id, label FROM invites WHERE redeemed_by = ?")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Spends the invite and creates its account in one transaction, and returns the account's id.
    /// The invite is checked first, so only someone holding a valid invite learns whether a
    /// username is taken (M3 spec D12).
    pub async fn redeem_invite(
        &self,
        code_hash: &[u8; 32],
        username: &str,
        password_phc: &str,
        now: i64,
    ) -> Result<i64> {
        let mut tx = self.begin().await?;
        let invite: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM invites WHERE code_hash = ? AND redeemed_at IS NULL \
             AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(&code_hash[..])
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((invite_id,)) = invite else {
            return Err(Error::InviteInvalid);
        };
        let account = sqlx::query(
            "INSERT INTO accounts (username, password_phc, created_at) VALUES (?, ?, ?)",
        )
        .bind(username)
        .bind(password_phc)
        .bind(now)
        .execute(&mut *tx)
        .await;
        let account_id = match account {
            Ok(r) => r.last_insert_rowid(),
            Err(e) if is_unique(&e) => return Err(Error::UsernameTaken),
            Err(e) => return Err(e.into()),
        };
        sqlx::query("UPDATE invites SET redeemed_by = ?, redeemed_at = ? WHERE id = ?")
            .bind(account_id)
            .bind(now)
            .bind(invite_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(account_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::Error;
    use crate::test_util::{NOW, h, store, warm};

    #[tokio::test]
    async fn an_invite_redeems_once() {
        let (_d, s) = store().await;
        s.create_invite(&h(1), Some("bob"), NOW, None)
            .await
            .unwrap();
        let id = s
            .redeem_invite(&h(1), "bob", "$phc", NOW + 1)
            .await
            .unwrap();
        assert!(matches!(
            s.redeem_invite(&h(1), "bob2", "$phc", NOW + 2).await,
            Err(Error::InviteInvalid)
        ));
        let rows = s.list_invites().await.unwrap();
        assert_eq!(rows[0].label.as_deref(), Some("bob"));
        assert_eq!(rows[0].redeemed_by.as_deref(), Some("bob"));
        assert_eq!(rows[0].redeemed_at, Some(NOW + 1));
        assert_eq!(s.account_by_id(id).await.unwrap().unwrap().username, "bob");
    }

    #[tokio::test]
    async fn an_account_names_the_invite_it_came_in_by() {
        // For the server's line on a new account (the hosting plan's final review B, Minor 2).
        let (_d, s) = store().await;
        let first = s
            .create_invite(&h(1), Some("for Mai"), NOW, None)
            .await
            .unwrap();
        let second = s.create_invite(&h(2), None, NOW, None).await.unwrap();
        let mai = s.redeem_invite(&h(1), "Mai", "$phc", NOW).await.unwrap();
        let ren = s.redeem_invite(&h(2), "Ren", "$phc", NOW).await.unwrap();
        assert_eq!(
            s.invite_of_account(mai).await.unwrap(),
            Some((first, Some("for Mai".into())))
        );
        assert_eq!(
            s.invite_of_account(ren).await.unwrap(),
            Some((second, None))
        );
        assert_eq!(s.invite_of_account(ren + 1).await.unwrap(), None);
    }

    #[tokio::test]
    async fn unknown_revoked_and_expired_invites_are_invalid() {
        let (_d, s) = store().await;
        assert!(matches!(
            s.redeem_invite(&h(9), "x1x", "$p", NOW).await,
            Err(Error::InviteInvalid)
        ));
        let id = s.create_invite(&h(2), None, NOW, None).await.unwrap();
        assert!(s.revoke_invite(id, NOW).await.unwrap());
        assert!(!s.revoke_invite(id, NOW).await.unwrap(), "already revoked");
        assert!(matches!(
            s.redeem_invite(&h(2), "x2x", "$p", NOW).await,
            Err(Error::InviteInvalid)
        ));
        s.create_invite(&h(3), None, NOW, Some(NOW + 10))
            .await
            .unwrap();
        assert!(matches!(
            s.redeem_invite(&h(3), "x3x", "$p", NOW + 10).await,
            Err(Error::InviteInvalid)
        ));
        assert!(s.account_by_username("x3x").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_taken_username_leaves_the_invite_unused() {
        let (_d, s) = store().await;
        s.create_invite(&h(1), None, NOW, None).await.unwrap();
        s.create_invite(&h(2), None, NOW, None).await.unwrap();
        s.redeem_invite(&h(1), "Alice", "$p", NOW).await.unwrap();
        assert!(matches!(
            s.redeem_invite(&h(2), "alice", "$p", NOW).await,
            Err(Error::UsernameTaken)
        ));
        s.redeem_invite(&h(2), "alice2", "$p", NOW).await.unwrap();
    }

    #[tokio::test]
    async fn a_redeemed_invite_cant_be_revoked() {
        let (_d, s) = store().await;
        let id = s.create_invite(&h(1), None, NOW, None).await.unwrap();
        s.redeem_invite(&h(1), "bob", "$p", NOW).await.unwrap();
        assert!(!s.revoke_invite(id, NOW).await.unwrap());
    }

    #[tokio::test]
    async fn an_invite_works_until_it_expires() {
        let (_d, s) = store().await;
        s.create_invite(&h(4), None, NOW, Some(NOW + 10))
            .await
            .unwrap();
        s.redeem_invite(&h(4), "early", "$p", NOW + 9)
            .await
            .unwrap();
        s.create_invite(&h(5), None, NOW, Some(NOW + 10))
            .await
            .unwrap();
        assert!(matches!(
            s.redeem_invite(&h(5), "late", "$p", NOW + 1_000).await,
            Err(Error::InviteInvalid)
        ));
    }

    #[tokio::test]
    async fn concurrent_redeems_of_one_invite_make_one_account() {
        let (_d, s) = store().await;
        s.create_invite(&h(1), None, NOW, None).await.unwrap();
        let code = h(1);
        warm(&s).await;
        let (x, y) = tokio::join!(
            s.redeem_invite(&code, "ann", "$p", NOW),
            s.redeem_invite(&code, "ben", "$p", NOW)
        );
        assert_eq!(x.is_ok() as u8 + y.is_ok() as u8, 1);
        assert!(matches!(x.err().or(y.err()), Some(Error::InviteInvalid)));
        assert_eq!(s.list_accounts().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn invite_is_open_answers_without_spending() {
        let (_d, s) = store().await;
        assert!(!s.invite_is_open(&h(1), NOW).await.unwrap(), "unknown");
        s.create_invite(&h(1), None, NOW, Some(NOW + 10))
            .await
            .unwrap();
        assert!(s.invite_is_open(&h(1), NOW + 9).await.unwrap());
        assert!(!s.invite_is_open(&h(1), NOW + 10).await.unwrap(), "expired");
        assert!(s.invite_is_open(&h(1), NOW).await.unwrap(), "still unspent");
        s.redeem_invite(&h(1), "bob", "$p", NOW).await.unwrap();
        assert!(!s.invite_is_open(&h(1), NOW).await.unwrap(), "spent");
        let id = s.create_invite(&h(2), None, NOW, None).await.unwrap();
        s.revoke_invite(id, NOW).await.unwrap();
        assert!(!s.invite_is_open(&h(2), NOW).await.unwrap(), "revoked");
    }
}
