//! Who is in `WORK` (M3 spec D17): at most `max` accounts, and one connection per account. A
//! second login replaces the first, which is told to close, and so is the connection of an account
//! the host bans (M4 hosting spec §8, HA5). The gate and the account API share it (`Arc<Online>`):
//! the API's server facts count the players online (M4 hosting spec §3.8), and the console's
//! `online` lists them, each with the character it plays (§3.3).
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
    accounts: HashMap<i64, Place>,
}

/// An account's place: its connection, and what the console's `online` shows of it.
struct Place {
    /// The connection's id.
    conn: u64,
    /// Fires when the registry tells the connection to close.
    closing: oneshot::Sender<Closing>,
    username: String,
    /// When it connected, in Unix milliseconds.
    since_ms: i64,
    /// Its character in the world, once the world has loaded it a scene.
    playing: Option<Playing>,
}

/// A character in the world, as the console's `online` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    pub character: String,
    /// The scene the world last loaded it.
    pub scene: i64,
}

/// An account connected, in the lobby or in the world: never its address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    pub account: i64,
    pub username: String,
    /// When it connected, in Unix milliseconds.
    pub since_ms: i64,
    /// Its character, in the world; `None` in the lobby.
    pub playing: Option<Playing>,
}

/// Why the registry tells a connection to close. Either way it has lost its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    /// A newer login to the same account has its place.
    Replaced,
    /// The host banned the account.
    Banned,
}

#[derive(Debug)]
pub enum Join {
    /// The connection's id, to leave with, and the receiver that fires when the registry tells it
    /// to close: a newer login replaces it, or the account is banned.
    Joined(u64, oneshot::Receiver<Closing>),
    Full,
}

impl Online {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Takes a place for `account`, named `username`, connecting at `now_ms` (Unix ms). An account
    /// already online keeps its place, and its older connection is told to close.
    pub fn join(&self, account: i64, username: &str, now_ms: i64) -> Join {
        let mut inner = self.lock();
        if !inner.accounts.contains_key(&account) && inner.accounts.len() >= self.max {
            return Join::Full;
        }
        inner.next += 1;
        let conn = inner.next;
        let (closing, rx) = oneshot::channel();
        let place = Place {
            conn,
            closing,
            username: username.to_owned(),
            since_ms: now_ms,
            playing: None,
        };
        if let Some(older) = inner.accounts.insert(account, place) {
            let _ = older.closing.send(Closing::Replaced);
        }
        Join::Joined(conn, rx)
    }

    /// Notes that connection `conn` plays `playing` in the world, now that the world has loaded it
    /// its scene: when it enters, and again on a teleport to another scene. Only while `conn` is
    /// still the account's connection.
    pub fn playing(&self, account: i64, conn: u64, playing: Playing) {
        let mut inner = self.lock();
        if let Some(place) = inner.accounts.get_mut(&account)
            && place.conn == conn
        {
            place.playing = Some(playing);
        }
    }

    /// Tells the account's connection, in the lobby or in the world, to close because the host
    /// banned the account (M4 hosting spec §8, HA5), and frees its place at once, as a newer login
    /// takes a replaced one's. The console's `ban` calls it after storing the ban and before it
    /// sends the world `WorldIn::Ban`, which reaches a player in the world alone. False when the
    /// account had no connection.
    pub fn ban(&self, account: i64) -> bool {
        let mut inner = self.lock();
        match inner.accounts.remove(&account) {
            Some(place) => {
                let _ = place.closing.send(Closing::Banned);
                true
            }
            None => false,
        }
    }

    /// Gives the place back, only if `id` is still the account's connection.
    pub fn leave(&self, account: i64, id: u64) {
        let mut inner = self.lock();
        if inner
            .accounts
            .get(&account)
            .is_some_and(|place| place.conn == id)
        {
            inner.accounts.remove(&account);
        }
    }

    /// The accounts connected, in the lobby or the world.
    pub fn count(&self) -> usize {
        self.lock().accounts.len()
    }

    /// The accounts connected, by username in any case.
    pub fn list(&self) -> Vec<Connected> {
        let mut list: Vec<Connected> = self
            .lock()
            .accounts
            .iter()
            .map(|(&account, place)| Connected {
                account,
                username: place.username.clone(),
                since_ms: place.since_ms,
                playing: place.playing.clone(),
            })
            .collect();
        list.sort_by_cached_key(|c| (c.username.to_ascii_lowercase(), c.account));
        list
    }

    /// The most accounts that may be connected at once (`[server] max_players`).
    pub fn max(&self) -> usize {
        self.max
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(j: Join) -> (u64, oneshot::Receiver<Closing>) {
        match j {
            Join::Joined(id, rx) => (id, rx),
            Join::Full => panic!("full"),
        }
    }

    #[test]
    fn the_cap_counts_accounts_not_connections() {
        let o = Online::new(2);
        let (_a, _ra) = joined(o.join(1, "a", 0));
        let (_b, _rb) = joined(o.join(2, "b", 0));
        assert!(matches!(o.join(3, "c", 0), Join::Full));
        // Account 1 again: its place, not a new one.
        let (_a2, _ra2) = joined(o.join(1, "a", 0));
        assert_eq!(o.count(), 2);
    }

    #[test]
    fn a_second_login_tells_the_first_to_close() {
        let o = Online::new(1);
        let (_a, mut first) = joined(o.join(7, "g", 0));
        assert!(first.try_recv().is_err());
        let (_a2, mut second) = joined(o.join(7, "g", 0));
        assert_eq!(
            first.try_recv(),
            Ok(Closing::Replaced),
            "the older connection was told"
        );
        assert!(second.try_recv().is_err());
    }

    #[test]
    fn a_ban_tells_the_accounts_connection_to_close_and_frees_its_place() {
        // M4 hosting spec §8, HA5: what the console's `ban` does for a player in the lobby, which
        // the world's own ban never reaches.
        let o = Online::new(1);
        let (_a, mut first) = joined(o.join(7, "g", 0));
        assert!(!o.ban(8), "an account that isn't connected");
        assert!(first.try_recv().is_err());
        assert!(o.ban(7));
        assert_eq!(first.try_recv(), Ok(Closing::Banned));
        assert_eq!(o.count(), 0, "its place is free at once");
        assert!(!o.ban(7), "told once");
        let (_b, _rb) = joined(o.join(8, "h", 0));
    }

    #[test]
    fn only_the_current_connection_gives_the_place_back() {
        let o = Online::new(1);
        let (old, _r) = joined(o.join(7, "g", 0));
        let (new, _r2) = joined(o.join(7, "g", 0));
        o.leave(7, old);
        assert_eq!(o.count(), 1, "the replaced connection's leave is a no-op");
        assert!(matches!(o.join(8, "h", 0), Join::Full));
        o.leave(7, new);
        assert_eq!(o.count(), 0);
        let (_c, _r3) = joined(o.join(8, "h", 0));
    }

    fn connected(
        account: i64,
        username: &str,
        since_ms: i64,
        playing: Option<Playing>,
    ) -> Connected {
        Connected {
            account,
            username: username.into(),
            since_ms,
            playing,
        }
    }

    fn lin(scene: i64) -> Playing {
        Playing {
            character: "Lin".into(),
            scene,
        }
    }

    #[test]
    fn the_list_names_each_account_with_its_character_and_since_when() {
        // The console's `online` (M4 hosting spec §3.3): by username in any case, each with the
        // character it plays, if any, and when it connected; never an address.
        let o = Online::new(4);
        let (mai, _rm) = joined(o.join(2, "mai", 2_000));
        let (_bob, _rb) = joined(o.join(1, "Bob", 1_000));
        assert_eq!(
            o.list(),
            [
                connected(1, "Bob", 1_000, None),
                connected(2, "mai", 2_000, None)
            ]
        );
        o.playing(2, mai, lin(5));
        assert_eq!(o.list()[1], connected(2, "mai", 2_000, Some(lin(5))));
        // A cross-scene teleport loads another scene: the newest is the one listed.
        o.playing(2, mai, lin(7));
        assert_eq!(o.list()[1].playing, Some(lin(7)));
        // A newer login is listed from its own start, in the lobby; the older connection's word
        // comes too late to count.
        let (newer, _rn) = joined(o.join(2, "mai", 3_000));
        o.playing(2, mai, lin(9));
        assert_eq!(o.list()[1], connected(2, "mai", 3_000, None));
        o.playing(2, newer, lin(5));
        assert_eq!(o.list()[1].playing, Some(lin(5)));
        // Gone once it leaves, or once it's banned.
        o.leave(2, newer);
        assert!(o.ban(1));
        o.playing(1, 1, lin(5));
        assert_eq!(o.list(), []);
    }
}
