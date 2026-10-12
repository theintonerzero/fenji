//! The account flows (M3 spec D3, D6, D12, D13): registering with an invite, signing in and out,
//! and issuing and spending game tokens. Every attempt goes through the limiter first; password
//! hashing runs on the blocking pool, at most four at a time, and never for an invite that can't
//! be redeemed. Nothing here logs or shows a password, token, ticket or invite code.
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fenji_auth::invite::InviteCode;
use fenji_auth::limiter::{Attempt, Limiter, LimiterConfig, Verdict};
use fenji_auth::password;
use fenji_auth::rules::{self, RuleError};
use fenji_auth::token::Token;
use fenji_store::{Account, Store};
use tokio::sync::Semaphore;

use crate::clock::Clock;
use crate::config::AuthConfig;

/// The limiter's name for registrations: a NUL first, so no username can match it. Failed
/// registrations count against their address like failed sign-ins.
const REGISTER: &str = "\0register";

#[derive(Debug, thiserror::Error)]
pub enum AccountsError {
    #[error("the invite is not valid")]
    InvalidInvite,
    #[error("that username is taken")]
    UsernameTaken,
    #[error("{0}")]
    Rule(RuleError),
    #[error("wrong username or password")]
    BadCredentials,
    #[error("too many attempts; try again in {} s", .0.as_secs().max(1))]
    TryLater(Duration),
    #[error("not signed in")]
    BadSession,
    #[error("internal error")]
    Internal(String),
}

impl From<fenji_store::Error> for AccountsError {
    fn from(e: fenji_store::Error) -> Self {
        match e {
            fenji_store::Error::InviteInvalid => Self::InvalidInvite,
            fenji_store::Error::UsernameTaken => Self::UsernameTaken,
            other => Self::Internal(other.to_string()),
        }
    }
}

/// A session after signing in. Its `Debug` doesn't show the token.
#[derive(Clone)]
pub struct SessionGrant {
    pub token: String,
    pub expires_at: i64,
}

impl std::fmt::Debug for SessionGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionGrant")
            .field("token", &"…")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// A game token for the launcher to hand the client. Its `Debug` doesn't show the ticket.
#[derive(Clone)]
pub struct GameGrant {
    pub ticket: String,
    pub expires_in_secs: u64,
    pub account_id: i64,
    pub username: String,
}

impl std::fmt::Debug for GameGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameGrant")
            .field("ticket", &"…")
            .field("expires_in_secs", &self.expires_in_secs)
            .field("account_id", &self.account_id)
            .field("username", &self.username)
            .finish()
    }
}

pub struct Accounts {
    store: Store,
    limiter: Mutex<Limiter>,
    hashing: Semaphore,
    hashes: AtomicU64,
    cfg: AuthConfig,
    clock: Arc<dyn Clock>,
}

impl Accounts {
    pub fn new(store: Store, cfg: AuthConfig, clock: Arc<dyn Clock>) -> Self {
        Self {
            store,
            limiter: Mutex::new(Limiter::new(LimiterConfig::default())),
            hashing: Semaphore::new(4),
            hashes: AtomicU64::new(0),
            cfg,
            clock,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// How many argon2 hashes and checks have run (for tests).
    pub fn hashes(&self) -> u64 {
        self.hashes.load(Ordering::Relaxed)
    }

    fn begin(&self, name: &str, addr: IpAddr) -> Result<Attempt, AccountsError> {
        let verdict = self
            .limiter
            .lock()
            .map_err(|_| AccountsError::Internal("limiter poisoned".into()))?
            .begin(name, addr, Instant::now());
        match verdict {
            Verdict::Allow(a) => Ok(a),
            Verdict::Locked { retry_after } | Verdict::SlowDown { retry_after } => {
                Err(AccountsError::TryLater(retry_after))
            }
        }
    }

    fn succeed(&self, attempt: Attempt) {
        if let Ok(mut l) = self.limiter.lock() {
            l.succeed(attempt);
        }
    }

    /// Runs argon2 work on the blocking pool, at most four at a time.
    async fn hashing<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, AccountsError> {
        let _permit = self
            .hashing
            .acquire()
            .await
            .map_err(|e| AccountsError::Internal(e.to_string()))?;
        self.hashes.fetch_add(1, Ordering::Relaxed);
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| AccountsError::Internal(e.to_string()))
    }

    /// Creates an account with an invite. The invite is checked before the username, and before
    /// any hashing, so a fake code costs little and teaches nothing about usernames (M3 spec D12).
    pub async fn register(
        &self,
        invite: &str,
        username: &str,
        password_: &str,
        addr: IpAddr,
    ) -> Result<i64, AccountsError> {
        let attempt = self.begin(REGISTER, addr)?;
        let code = InviteCode::parse(invite).ok_or(AccountsError::InvalidInvite)?;
        let now = self.clock.now_ms();
        if !self.store.invite_is_open(&code.hash(), now).await? {
            return Err(AccountsError::InvalidInvite);
        }
        rules::check_username(username).map_err(AccountsError::Rule)?;
        rules::check_password(password_).map_err(AccountsError::Rule)?;
        let pw = password_.to_owned();
        let phc = self
            .hashing(move || password::hash(&pw))
            .await?
            .map_err(|e| AccountsError::Internal(e.to_string()))?;
        let id = self
            .store
            .redeem_invite(&code.hash(), username, &phc, now)
            .await?;
        self.succeed(attempt);
        Ok(id)
    }

    /// Signs in: a session token on success, and the same error for a wrong password and an unknown
    /// username, which takes as long to decide.
    pub async fn sign_in(
        &self,
        username: &str,
        password_: &str,
        addr: IpAddr,
    ) -> Result<SessionGrant, AccountsError> {
        let attempt = self.begin(username, addr)?;
        let account = self.store.account_by_username(username).await?;
        let pw = password_.to_owned();
        let ok = match account.clone() {
            Some(a) => self
                .hashing(move || password::verify(&pw, &a.password_phc))
                .await?
                .map_err(|e| AccountsError::Internal(e.to_string()))?,
            None => {
                self.hashing(move || password::verify_dummy(&pw)).await?;
                false
            }
        };
        let Some(account) = account.filter(|_| ok) else {
            return Err(AccountsError::BadCredentials);
        };
        self.succeed(attempt);
        let now = self.clock.now_ms();
        self.store.record_sign_in(account.id, now).await?;
        let token = Token::generate().map_err(|e| AccountsError::Internal(e.to_string()))?;
        let ttl = i64::try_from(self.cfg.session_ttl_secs).unwrap_or(i64::MAX / 1000) * 1000;
        let expires_at = now.saturating_add(ttl);
        self.store
            .create_session(&token.hash(), account.id, now, expires_at)
            .await?;
        Ok(SessionGrant {
            token: token.as_string(),
            expires_at,
        })
    }

    async fn session(&self, session: &str) -> Result<Account, AccountsError> {
        let t = Token::parse(session).ok_or(AccountsError::BadSession)?;
        let id = self
            .store
            .session_account(&t.hash(), self.clock.now_ms())
            .await?
            .ok_or(AccountsError::BadSession)?;
        self.store
            .account_by_id(id)
            .await?
            .ok_or(AccountsError::BadSession)
    }

    pub async fn sign_out(&self, session: &str) -> Result<(), AccountsError> {
        let t = Token::parse(session).ok_or(AccountsError::BadSession)?;
        self.store.delete_session(&t.hash()).await?;
        Ok(())
    }

    /// A single-use game token for the session's account, valid for `game_token_ttl_secs`.
    pub async fn issue_game_token(&self, session: &str) -> Result<GameGrant, AccountsError> {
        let account = self.session(session).await?;
        let token = Token::generate().map_err(|e| AccountsError::Internal(e.to_string()))?;
        let ttl = self.cfg.game_token_ttl_secs;
        let expires_at = self
            .clock
            .now_ms()
            .saturating_add(i64::try_from(ttl).unwrap_or(i64::MAX / 1000) * 1000);
        self.store
            .create_game_token(&token.hash(), account.id, expires_at)
            .await?;
        Ok(GameGrant {
            ticket: token.as_string(),
            expires_in_secs: ttl,
            account_id: account.id,
            username: account.username,
        })
    }

    /// The account a ticket names, spending the ticket: `None` for anything that isn't a live,
    /// unspent Fenji game token.
    pub async fn consume_game_token(&self, ticket: &str) -> Result<Option<Account>, AccountsError> {
        let Some(t) = Token::parse(ticket) else {
            return Ok(None);
        };
        match self
            .store
            .consume_game_token(&t.hash(), self.clock.now_ms())
            .await?
        {
            Some(id) => Ok(self.store.account_by_id(id).await?),
            None => Ok(None),
        }
    }

    /// Drops expired sessions and spent or expired game tokens.
    pub async fn purge(&self) -> Result<u64, AccountsError> {
        Ok(self.store.purge_expired(self.clock.now_ms()).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;

    use fenji_auth::invite::InviteCode;
    use fenji_store::Store;

    use super::*;
    use crate::clock::{Clock, TestClock};
    use crate::config::AuthConfig;

    const ADDR: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3));

    async fn setup() -> (tempfile::TempDir, Accounts, TestClock, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("f.db")).await.unwrap();
        let clock = TestClock::at(1_800_000_000_000);
        let code = InviteCode::generate().unwrap();
        store
            .create_invite(&code.hash(), Some("t"), clock.now_ms(), None)
            .await
            .unwrap();
        let a = Accounts::new(store, AuthConfig::default(), Arc::new(clock.clone()));
        (dir, a, clock, code.display())
    }

    #[tokio::test]
    async fn register_sign_in_and_get_a_single_use_game_token() {
        let (_d, a, _c, code) = setup().await;
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
        let s = a.sign_in("TESTER", "a long password", ADDR).await.unwrap();
        let g = a.issue_game_token(&s.token).await.unwrap();
        assert!(g.ticket.starts_with("fenji1-"));
        assert_eq!((g.username.as_str(), g.expires_in_secs), ("tester", 60));
        let account = a.consume_game_token(&g.ticket).await.unwrap().unwrap();
        assert_eq!(account.username, "tester");
        assert!(a.consume_game_token(&g.ticket).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn an_invalid_invite_is_reported_before_the_username_and_without_hashing() {
        let (_d, a, _c, code) = setup().await;
        a.register(&code, "taken", "a long password", ADDR)
            .await
            .unwrap();
        let before = a.hashes();
        let e = a
            .register("0000-0000-0000-0000", "taken", "a long password", ADDR)
            .await
            .unwrap_err();
        assert!(matches!(e, AccountsError::InvalidInvite), "{e:?}");
        assert!(matches!(
            a.register("not a code", "x1x", "a long password", ADDR)
                .await,
            Err(AccountsError::InvalidInvite)
        ));
        assert_eq!(a.hashes(), before, "no argon2 work for a bad invite");
    }

    #[tokio::test]
    async fn bad_invites_count_against_the_address() {
        let (_d, a, _c, _code) = setup().await;
        for _ in 0..5 {
            let _ = a
                .register("0000-0000-0000-0000", "x1x", "a long password", ADDR)
                .await;
        }
        assert!(matches!(
            a.register("0000-0000-0000-0000", "x1x", "a long password", ADDR)
                .await,
            Err(AccountsError::TryLater(_))
        ));
    }

    #[tokio::test]
    async fn rules_are_checked_before_hashing() {
        let (_d, a, _c, code) = setup().await;
        let before = a.hashes();
        assert!(matches!(
            a.register(&code, "a:b", "a long password", ADDR).await,
            Err(AccountsError::Rule(_))
        ));
        assert!(matches!(
            a.register(&code, "tester", "short", ADDR).await,
            Err(AccountsError::Rule(_))
        ));
        assert_eq!(a.hashes(), before);
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn wrong_passwords_and_unknown_users_look_the_same() {
        let (_d, a, _c, code) = setup().await;
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
        let e1 = a
            .sign_in("tester", "wrong password!", ADDR)
            .await
            .unwrap_err();
        let e2 = a
            .sign_in("nobody", "wrong password!", ADDR)
            .await
            .unwrap_err();
        assert_eq!(e1.to_string(), e2.to_string());
        assert!(matches!(e2, AccountsError::BadCredentials));
    }

    #[tokio::test]
    async fn five_failures_lock_the_pair() {
        let (_d, a, _c, code) = setup().await;
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
        for _ in 0..5 {
            let _ = a.sign_in("tester", "wrong password!", ADDR).await;
        }
        assert!(matches!(
            a.sign_in("tester", "a long password", ADDR).await,
            Err(AccountsError::TryLater(_))
        ));
    }

    #[tokio::test]
    async fn successes_never_lock_anyone_out() {
        let (_d, a, _c, code) = setup().await;
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
        for _ in 0..30 {
            a.sign_in("tester", "a long password", ADDR).await.unwrap();
        }
    }

    #[tokio::test]
    async fn tokens_expire_and_sessions_end() {
        let (_d, a, c, code) = setup().await;
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
        let s = a.sign_in("tester", "a long password", ADDR).await.unwrap();
        let g = a.issue_game_token(&s.token).await.unwrap();
        c.advance(60_000);
        assert!(a.consume_game_token(&g.ticket).await.unwrap().is_none());
        a.sign_out(&s.token).await.unwrap();
        assert!(matches!(
            a.issue_game_token(&s.token).await,
            Err(AccountsError::BadSession)
        ));
    }

    #[tokio::test]
    async fn garbage_tickets_are_simply_unknown() {
        let (_d, a, _c, _) = setup().await;
        for t in ["", "fenji-dev", "{\"auth_type\":\"mocked\"}", "fenji1-zz"] {
            assert!(a.consume_game_token(t).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn grants_never_show_their_secrets() {
        let (_d, a, _c, code) = setup().await;
        a.register(&code, "tester", "a long password", ADDR)
            .await
            .unwrap();
        let s = a.sign_in("tester", "a long password", ADDR).await.unwrap();
        let g = a.issue_game_token(&s.token).await.unwrap();
        assert!(!format!("{s:?}").contains(&s.token[7..20]), "{s:?}");
        assert!(!format!("{g:?}").contains(&g.ticket[7..20]), "{g:?}");
    }
}
