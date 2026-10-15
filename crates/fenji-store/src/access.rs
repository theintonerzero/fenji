//! Ops and bans (M4 hosting spec §8, HA5): the accounts the host made admins from the console,
//! besides those `[access].admins` names, and the accounts the host banned, with a reason and
//! since when. Both go with their account.
use sqlx::FromRow;

use crate::{Result, Store};

/// A ban, as `bans` lists it and the account API reports it.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Ban {
    pub account_id: i64,
    pub username: String,
    /// The host's words, which the banned player is shown; may be empty.
    pub reason: String,
    /// Unix milliseconds.
    pub since: i64,
}

impl Store {
    /// Makes the account an op: false when it was one already.
    pub async fn op(&self, account_id: i64) -> Result<bool> {
        let r = sqlx::query("INSERT INTO ops (account_id) VALUES (?) ON CONFLICT DO NOTHING")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Takes op away from the account: false when it wasn't one.
    pub async fn deop(&self, account_id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM ops WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn is_op(&self, account_id: i64) -> Result<bool> {
        let op: Option<i64> = sqlx::query_scalar("SELECT account_id FROM ops WHERE account_id = ?")
            .bind(account_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(op.is_some())
    }

    /// The ops' usernames, in order (without regard to case).
    pub async fn ops(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT a.username FROM ops o JOIN accounts a ON a.id = o.account_id \
             ORDER BY a.username",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Bans the account, from `since`, for `reason`: true when it wasn't banned. Banning it again
    /// changes the reason only, so a ban keeps the time it began.
    pub async fn ban(&self, account_id: i64, reason: &str, since: i64) -> Result<bool> {
        let mut tx = self.begin().await?;
        let before: Option<i64> =
            sqlx::query_scalar("SELECT account_id FROM bans WHERE account_id = ?")
                .bind(account_id)
                .fetch_optional(&mut *tx)
                .await?;
        sqlx::query(
            "INSERT INTO bans (account_id, reason, since) VALUES (?, ?, ?) \
             ON CONFLICT (account_id) DO UPDATE SET reason = excluded.reason",
        )
        .bind(account_id)
        .bind(reason)
        .bind(since)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(before.is_none())
    }

    /// Lifts the account's ban: false when it had none.
    pub async fn pardon(&self, account_id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM bans WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Every ban, in the order of the accounts' usernames (without regard to case).
    pub async fn bans(&self) -> Result<Vec<Ban>> {
        Ok(sqlx::query_as(
            "SELECT b.account_id, a.username, b.reason, b.since \
             FROM bans b JOIN accounts a ON a.id = b.account_id ORDER BY a.username",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// The account's ban, if it has one.
    pub async fn ban_of(&self, account_id: i64) -> Result<Option<Ban>> {
        Ok(sqlx::query_as(
            "SELECT b.account_id, a.username, b.reason, b.since \
             FROM bans b JOIN accounts a ON a.id = b.account_id WHERE b.account_id = ?",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::Ban;
    use crate::test_util::{NOW, account, h, store};

    #[tokio::test]
    async fn ops_are_made_listed_and_taken_away() {
        let (_d, s) = store().await;
        let bob = account(&s, 1, "bob").await;
        let amy = account(&s, 2, "Amy").await;
        assert!(!s.is_op(bob).await.unwrap());
        assert!(s.op(bob).await.unwrap());
        assert!(!s.op(bob).await.unwrap(), "an op already");
        assert!(s.op(amy).await.unwrap());
        assert!(s.is_op(bob).await.unwrap());
        assert_eq!(s.ops().await.unwrap(), ["Amy", "bob"]);
        assert!(s.deop(bob).await.unwrap());
        assert!(!s.deop(bob).await.unwrap(), "not an op any more");
        assert!(!s.is_op(bob).await.unwrap());
        assert_eq!(s.ops().await.unwrap(), ["Amy"]);
    }

    #[tokio::test]
    async fn a_ban_keeps_its_first_time_and_its_latest_reason_until_a_pardon() {
        let (_d, s) = store().await;
        let bob = account(&s, 1, "bob").await;
        let amy = account(&s, 2, "amy").await;
        assert_eq!(s.ban_of(bob).await.unwrap(), None);
        assert!(s.ban(bob, "spam", NOW).await.unwrap());
        assert!(
            !s.ban(bob, "spam and griefing", NOW + 5).await.unwrap(),
            "banned already"
        );
        assert!(s.ban(amy, "", NOW + 9).await.unwrap());
        let bobs = Ban {
            account_id: bob,
            username: "bob".into(),
            reason: "spam and griefing".into(),
            since: NOW,
        };
        assert_eq!(s.ban_of(bob).await.unwrap(), Some(bobs.clone()));
        let amys = Ban {
            account_id: amy,
            username: "amy".into(),
            reason: String::new(),
            since: NOW + 9,
        };
        assert_eq!(s.bans().await.unwrap(), [amys, bobs]);
        assert!(s.pardon(bob).await.unwrap());
        assert!(!s.pardon(bob).await.unwrap(), "no ban to lift");
        assert_eq!(s.ban_of(bob).await.unwrap(), None);
        assert_eq!(s.bans().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ops_and_bans_go_with_their_account() {
        let (_d, s) = store().await;
        let bob = account(&s, 1, "bob").await;
        s.op(bob).await.unwrap();
        s.ban(bob, "spam", NOW).await.unwrap();
        assert!(s.delete_account("bob").await.unwrap());
        assert!(s.ops().await.unwrap().is_empty());
        assert!(s.bans().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_banned_accounts_ticket_is_left_unspent() {
        // A ticket issued just before the ban doesn't let the account in after it.
        let (_d, s) = store().await;
        let bob = account(&s, 1, "bob").await;
        s.create_game_token(&h(5), bob, NOW + 60_000).await.unwrap();
        s.ban(bob, "spam", NOW).await.unwrap();
        assert_eq!(s.consume_game_token(&h(5), NOW + 1).await.unwrap(), None);
        s.pardon(bob).await.unwrap();
        assert_eq!(
            s.consume_game_token(&h(5), NOW + 2).await.unwrap(),
            Some(bob),
            "it wasn't spent"
        );
    }
}
