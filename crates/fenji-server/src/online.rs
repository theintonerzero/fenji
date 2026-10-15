//! Who is in `WORK` (M3 spec D17): at most `max` accounts, and one connection per account. A
//! second login replaces the first, which is told to close. The gate and the account API share it
//! (`Arc<Online>`): the API's server facts count the players online (M4 hosting spec §3.8).
use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;

pub struct Online {
    max: usize,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    accounts: HashMap<i64, (u64, oneshot::Sender<()>)>,
}

#[derive(Debug)]
pub enum Join {
    /// The connection's id, to leave with, and the receiver that fires when a newer login
    /// replaces it.
    Joined(u64, oneshot::Receiver<()>),
    Full,
}

impl Online {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Takes a place for `account`. An account already online keeps its place, and its older
    /// connection is told to close.
    pub fn join(&self, account: i64) -> Join {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.accounts.contains_key(&account) && inner.accounts.len() >= self.max {
            return Join::Full;
        }
        inner.next += 1;
        let id = inner.next;
        let (tx, rx) = oneshot::channel();
        if let Some((_, older)) = inner.accounts.insert(account, (id, tx)) {
            let _ = older.send(());
        }
        Join::Joined(id, rx)
    }

    /// Gives the place back, only if `id` is still the account's connection.
    pub fn leave(&self, account: i64, id: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner
            .accounts
            .get(&account)
            .is_some_and(|(cur, _)| *cur == id)
        {
            inner.accounts.remove(&account);
        }
    }

    /// The accounts connected, in the lobby or the world.
    pub fn count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .accounts
            .len()
    }

    /// The most accounts that may be connected at once (`[server] max_players`).
    pub fn max(&self) -> usize {
        self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(j: Join) -> (u64, oneshot::Receiver<()>) {
        match j {
            Join::Joined(id, rx) => (id, rx),
            Join::Full => panic!("full"),
        }
    }

    #[test]
    fn the_cap_counts_accounts_not_connections() {
        let o = Online::new(2);
        let (_a, _ra) = joined(o.join(1));
        let (_b, _rb) = joined(o.join(2));
        assert!(matches!(o.join(3), Join::Full));
        // Account 1 again: its place, not a new one.
        let (_a2, _ra2) = joined(o.join(1));
        assert_eq!(o.count(), 2);
    }

    #[test]
    fn a_second_login_tells_the_first_to_close() {
        let o = Online::new(1);
        let (_a, mut first) = joined(o.join(7));
        assert!(first.try_recv().is_err());
        let (_a2, mut second) = joined(o.join(7));
        assert!(first.try_recv().is_ok(), "the older connection was told");
        assert!(second.try_recv().is_err());
    }

    #[test]
    fn only_the_current_connection_gives_the_place_back() {
        let o = Online::new(1);
        let (old, _r) = joined(o.join(7));
        let (new, _r2) = joined(o.join(7));
        o.leave(7, old);
        assert_eq!(o.count(), 1, "the replaced connection's leave is a no-op");
        assert!(matches!(o.join(8), Join::Full));
        o.leave(7, new);
        assert_eq!(o.count(), 0);
        let (_c, _r3) = joined(o.join(8));
    }
}
