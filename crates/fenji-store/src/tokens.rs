use crate::{Result, Store};

impl Store {
    pub async fn create_session(
        &self,
        token_hash: &[u8; 32],
        account_id: i64,
        now: i64,
        expires_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&token_hash[..])
        .bind(account_id)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The account of a live session.
    pub async fn session_account(&self, token_hash: &[u8; 32], now: i64) -> Result<Option<i64>> {
        let r: Option<(i64,)> = sqlx::query_as(
            "SELECT account_id FROM sessions WHERE token_hash = ? AND expires_at > ?",
        )
        .bind(&token_hash[..])
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(|(a,)| a))
    }

    pub async fn delete_session(&self, token_hash: &[u8; 32]) -> Result<bool> {
        let r = sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(&token_hash[..])
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Stores a new game token for the account, in place of its unspent ones: an account holds at
    /// most one live ticket, the latest, so issuing can't pile up rows until the hourly purge.
    pub async fn create_game_token(
        &self,
        token_hash: &[u8; 32],
        account_id: i64,
        expires_at: i64,
    ) -> Result<()> {
        let mut tx = self.begin().await?;
        sqlx::query("DELETE FROM game_tokens WHERE account_id = ? AND used_at IS NULL")
            .bind(account_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO game_tokens (token_hash, account_id, expires_at) VALUES (?, ?, ?)",
        )
        .bind(&token_hash[..])
        .bind(account_id)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Spends a game token and returns its account. It is one statement, so two connections that
    /// present the same token can't both get the account (M3 spec D3).
    pub async fn consume_game_token(&self, token_hash: &[u8; 32], now: i64) -> Result<Option<i64>> {
        let r: Option<(i64,)> = sqlx::query_as(
            "UPDATE game_tokens SET used_at = ? \
             WHERE token_hash = ? AND used_at IS NULL AND expires_at > ? RETURNING account_id",
        )
        .bind(now)
        .bind(&token_hash[..])
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(|(a,)| a))
    }

    /// Drops expired sessions and every game token that is spent or expired; returns how many.
    pub async fn purge_expired(&self, now: i64) -> Result<u64> {
        let mut tx = self.begin().await?;
        let s = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        let t = sqlx::query("DELETE FROM game_tokens WHERE used_at IS NOT NULL OR expires_at <= ?")
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(s.rows_affected() + t.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::{NOW, account, h, store, warm};

    #[tokio::test]
    async fn sessions_expire_and_end() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        s.create_session(&h(5), a, NOW, NOW + 100).await.unwrap();
        assert_eq!(s.session_account(&h(5), NOW + 99).await.unwrap(), Some(a));
        assert_eq!(s.session_account(&h(5), NOW + 100).await.unwrap(), None);
        assert!(s.delete_session(&h(5)).await.unwrap());
        assert!(!s.delete_session(&h(5)).await.unwrap());
        assert_eq!(s.session_account(&h(5), NOW).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_new_game_token_replaces_the_accounts_unspent_ones() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let b = account(&s, 2, "eve").await;
        s.create_game_token(&h(10), b, NOW + 60_000).await.unwrap();
        for n in 20..30 {
            s.create_game_token(&h(n), a, NOW + 60_000).await.unwrap();
        }
        // Only the latest of bob's works; eve's is untouched.
        for n in 20..29 {
            assert_eq!(s.consume_game_token(&h(n), NOW).await.unwrap(), None, "{n}");
        }
        assert_eq!(s.consume_game_token(&h(29), NOW).await.unwrap(), Some(a));
        assert_eq!(s.consume_game_token(&h(10), NOW).await.unwrap(), Some(b));
        let rows: (i64,) = sqlx::query_as("SELECT count(*) FROM game_tokens WHERE account_id = ?")
            .bind(a)
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(
            rows.0, 1,
            "issuing doesn't pile up rows until the hourly purge"
        );
    }

    #[tokio::test]
    async fn a_game_token_is_consumed_once_and_only_in_time() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        s.create_game_token(&h(6), a, NOW + 60_000).await.unwrap();
        assert_eq!(s.consume_game_token(&h(6), NOW + 1).await.unwrap(), Some(a));
        assert_eq!(s.consume_game_token(&h(6), NOW + 2).await.unwrap(), None);
        s.create_game_token(&h(7), a, NOW + 60_000).await.unwrap();
        assert_eq!(
            s.consume_game_token(&h(7), NOW + 60_000).await.unwrap(),
            None
        );
        assert_eq!(s.consume_game_token(&h(8), NOW).await.unwrap(), None);
    }

    #[tokio::test]
    async fn concurrent_consumers_of_one_token_get_one_account() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        s.create_game_token(&h(6), a, NOW + 60_000).await.unwrap();
        let token = h(6);
        warm(&s).await;
        let (x, y) = tokio::join!(
            s.consume_game_token(&token, NOW),
            s.consume_game_token(&token, NOW)
        );
        assert_eq!(x.unwrap().is_some() as u8 + y.unwrap().is_some() as u8, 1);
    }

    #[tokio::test]
    async fn purge_drops_expired_sessions_and_spent_tokens() {
        let (_d, s) = store().await;
        let a = account(&s, 1, "bob").await;
        let b = account(&s, 9, "eve").await;
        s.create_session(&h(2), a, NOW, NOW + 10).await.unwrap();
        s.create_session(&h(3), a, NOW, NOW + 1_000).await.unwrap();
        // eve's expired token: bob's next one would replace an unspent token of his own.
        s.create_game_token(&h(4), b, NOW + 10).await.unwrap();
        s.create_game_token(&h(5), a, NOW + 1_000).await.unwrap();
        s.consume_game_token(&h(5), NOW).await.unwrap();
        s.create_game_token(&h(6), a, NOW + 1_000).await.unwrap(); // live and unused: kept
        assert_eq!(s.purge_expired(NOW + 20).await.unwrap(), 3);
        assert_eq!(s.session_account(&h(3), NOW + 20).await.unwrap(), Some(a));
        assert_eq!(
            s.consume_game_token(&h(6), NOW + 20).await.unwrap(),
            Some(a)
        );
    }
}
