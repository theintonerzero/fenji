//! The sign-in limiter (M3 spec D12). Attempts are counted in a sliding window per (username,
//! address) pair and per address, either of which refuses further attempts while it holds its
//! limit, and per username, which only slows down to one attempt every `slow_interval`: so nobody
//! can lock a player out by knowing their username.
//!
//! An attempt counts the moment it begins, and a success takes it back, so attempts made in
//! parallel are counted before their password checks finish. Addresses count by network: an IPv4
//! address alone, an IPv6 address by its /64, which one subscriber usually holds whole.
//!
//! Memory stays bounded without forgetting any count: past `max_keys` entries per kind, only the
//! entries whose attempts have all left the window are dropped, and at four times `max_keys` a
//! new key waits ([`Verdict::SlowDown`]) until old ones leave. During such a flood a newcomer
//! waits too, which is the price of never letting a flood reset anyone's count. It lives in memory, and a
//! restart forgets it, which suits a small server.
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::net::{IpAddr, Ipv6Addr};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LimiterConfig {
    pub pair_max: usize,
    pub addr_max: usize,
    pub user_max: usize,
    pub window: Duration,
    pub slow_interval: Duration,
    /// Keys per kind past which stale entries are dropped; at four times this, new keys wait.
    pub max_keys: usize,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            pair_max: 5,
            addr_max: 20,
            user_max: 10,
            window: Duration::from_secs(15 * 60),
            slow_interval: Duration::from_secs(2),
            max_keys: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Locked { retry_after: Duration },
    SlowDown { retry_after: Duration },
}

#[derive(Default)]
struct Entry {
    attempts: VecDeque<Instant>,
    last_attempt: Option<Instant>,
}

impl Entry {
    fn trim(&mut self, now: Instant, window: Duration) {
        while self
            .attempts
            .front()
            .is_some_and(|t| now.duration_since(*t) >= window)
        {
            self.attempts.pop_front();
        }
    }

    /// While this entry holds `max` attempts in the window: how long until the oldest attempt that
    /// keeps it there leaves.
    fn locked_for(&self, now: Instant, window: Duration, max: usize) -> Option<Duration> {
        let held = *self.attempts.get(self.attempts.len().checked_sub(max)?)?;
        Some(window.saturating_sub(now.duration_since(held)))
    }
}

/// The key an address counts under: IPv4 (an IPv4-mapped IPv6 address included) alone, any other
/// IPv6 address by its /64.
fn net_key(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V4(_) => addr,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(Ipv6Addr::from_bits(v6.to_bits() & !u128::from(u64::MAX))),
        },
    }
}

pub struct Limiter {
    cfg: LimiterConfig,
    pairs: HashMap<(String, IpAddr), Entry>,
    addrs: HashMap<IpAddr, Entry>,
    users: HashMap<String, Entry>,
}

impl Limiter {
    pub fn new(cfg: LimiterConfig) -> Self {
        Self {
            cfg,
            pairs: HashMap::new(),
            addrs: HashMap::new(),
            users: HashMap::new(),
        }
    }

    /// Starts an attempt. It is refused while its pair or its address holds its limit, or while
    /// its username is slowed down; an allowed attempt counts as a failure until
    /// [`Limiter::succeed`] takes it back.
    pub fn begin(&mut self, username: &str, addr: IpAddr, now: Instant) -> Verdict {
        let (user, net) = (username.to_ascii_lowercase(), net_key(addr));
        let (window, hard) = (self.cfg.window, self.cfg.max_keys.saturating_mul(4));
        if !has_room(&mut self.pairs, &(user.clone(), net), now, window, hard)
            || !has_room(&mut self.addrs, &net, now, window, hard)
            || !has_room(&mut self.users, &user, now, window, hard)
        {
            return Verdict::SlowDown {
                retry_after: FULL_WAIT,
            };
        }
        let cfg = &self.cfg;
        let pair = self.pairs.entry((user.clone(), net)).or_default();
        pair.trim(now, cfg.window);
        let pair_lock = pair.locked_for(now, cfg.window, cfg.pair_max);
        let a = self.addrs.entry(net).or_default();
        a.trim(now, cfg.window);
        let addr_lock = a.locked_for(now, cfg.window, cfg.addr_max);
        if let Some(retry_after) = pair_lock.into_iter().chain(addr_lock).max() {
            return Verdict::Locked { retry_after };
        }
        let u = self.users.entry(user.clone()).or_default();
        u.trim(now, cfg.window);
        if u.attempts.len() >= cfg.user_max
            && let Some(since) = u.last_attempt.map(|last| now.duration_since(last))
            && since < cfg.slow_interval
        {
            return Verdict::SlowDown {
                retry_after: cfg.slow_interval - since,
            };
        }
        u.attempts.push_back(now);
        u.last_attempt = Some(now);
        for e in [self.pairs.get_mut(&(user, net)), self.addrs.get_mut(&net)]
            .into_iter()
            .flatten()
        {
            e.attempts.push_back(now);
        }
        self.prune(now);
        Verdict::Allow
    }

    /// The attempt succeeded: its pair starts afresh, and its address and username get the attempt
    /// back.
    pub fn succeed(&mut self, username: &str, addr: IpAddr) {
        let (user, net) = (username.to_ascii_lowercase(), net_key(addr));
        self.pairs.remove(&(user.clone(), net));
        if let Some(a) = self.addrs.get_mut(&net) {
            a.attempts.pop_back();
        }
        if let Some(u) = self.users.get_mut(&user) {
            u.attempts.pop_back();
        }
    }

    /// Keys held across the three kinds (for tests).
    pub fn key_count(&self) -> usize {
        self.pairs.len() + self.addrs.len() + self.users.len()
    }

    fn prune(&mut self, now: Instant) {
        let (window, max) = (self.cfg.window, self.cfg.max_keys);
        prune_map(&mut self.pairs, now, window, max);
        prune_map(&mut self.addrs, now, window, max);
        prune_map(&mut self.users, now, window, max);
    }
}

/// How long a new key waits while a kind holds four times `max_keys` entries.
const FULL_WAIT: Duration = Duration::from_secs(60);

/// Whether `key` may be counted in `map`: it is there already, or the map is under `hard`, after
/// dropping entries whose attempts have all left the window.
fn has_room<K: Eq + Hash>(
    map: &mut HashMap<K, Entry>,
    key: &K,
    now: Instant,
    window: Duration,
    hard: usize,
) -> bool {
    if map.contains_key(key) || map.len() < hard {
        return true;
    }
    map.retain(|_, e| {
        e.trim(now, window);
        !e.attempts.is_empty()
    });
    map.len() < hard
}

/// Drops, past `max` keys, the entries whose attempts have all left the window. An entry with an
/// attempt in the window is never dropped: dropping one would forget its count and let its next
/// attempts through. [`has_room`] bounds the rest.
fn prune_map<K: Eq + Hash>(
    map: &mut HashMap<K, Entry>,
    now: Instant,
    window: Duration,
    max: usize,
) {
    if map.len() <= max {
        return;
    }
    map.retain(|_, e| {
        e.trim(now, window);
        !e.attempts.is_empty()
    });
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::{Duration, Instant};

    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, n))
    }

    fn limiter() -> Limiter {
        Limiter::new(LimiterConfig::default())
    }

    #[test]
    fn five_failures_lock_the_pair_but_not_another_address() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            assert_eq!(l.begin("alice", ip(1), t), Verdict::Allow);
        }
        assert!(matches!(l.begin("alice", ip(1), t), Verdict::Locked { .. }));
        assert_eq!(l.begin("alice", ip(2), t), Verdict::Allow);
    }

    #[test]
    fn the_lock_ends_and_success_clears_the_pair() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            l.begin("bob", ip(1), t);
        }
        let later = t + Duration::from_secs(15 * 60 + 1);
        for _ in 0..4 {
            assert_eq!(l.begin("bob", ip(1), later), Verdict::Allow);
        }
        assert_eq!(l.begin("bob", ip(1), later), Verdict::Allow);
        l.succeed("bob", ip(1));
        assert_eq!(
            l.begin("bob", ip(1), later),
            Verdict::Allow,
            "the success cleared the pair"
        );
    }

    #[test]
    fn twenty_failures_from_one_address_lock_the_address() {
        let mut l = limiter();
        let t = Instant::now();
        for i in 0..20 {
            l.begin(&format!("user{i}"), ip(9), t);
        }
        assert!(matches!(
            l.begin("someone-else", ip(9), t),
            Verdict::Locked { .. }
        ));
        assert_eq!(l.begin("someone-else", ip(8), t), Verdict::Allow);
    }

    #[test]
    fn many_failures_for_one_username_only_slow_it_down() {
        let mut l = limiter();
        let t = Instant::now();
        for i in 0..10 {
            l.begin("carol", ip(i), t);
        }
        // From a fresh address: never locked, but spaced 2 s apart.
        assert!(matches!(
            l.begin("carol", ip(100), t),
            Verdict::SlowDown { .. }
        ));
        assert_eq!(
            l.begin("carol", ip(100), t + Duration::from_secs(2)),
            Verdict::Allow
        );
        assert!(matches!(
            l.begin("carol", ip(101), t + Duration::from_secs(3)),
            Verdict::SlowDown { .. }
        ));
    }

    #[test]
    fn usernames_compare_case_insensitively() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            l.begin("Dave", ip(1), t);
        }
        assert!(matches!(l.begin("dave", ip(1), t), Verdict::Locked { .. }));
    }

    #[test]
    fn old_failures_leave_the_window() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..4 {
            l.begin("erin", ip(1), t);
        }
        let later = t + Duration::from_secs(15 * 60);
        l.begin("erin", ip(1), later);
        assert_eq!(
            l.begin("erin", ip(1), later),
            Verdict::Allow,
            "the four old ones left the window"
        );
    }

    #[test]
    fn parallel_attempts_count_before_their_results() {
        // Six sign-ins arrive together: each is counted when it starts, not when its password
        // check ends, so the sixth is refused although no result is in yet.
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            assert_eq!(l.begin("frank", ip(1), t), Verdict::Allow);
        }
        assert!(matches!(l.begin("frank", ip(1), t), Verdict::Locked { .. }));
    }

    #[test]
    fn a_success_refunds_its_attempt() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..20 {
            assert_eq!(l.begin("gina", ip(1), t), Verdict::Allow);
            l.succeed("gina", ip(1));
        }
        assert_eq!(
            l.begin("gina", ip(1), t),
            Verdict::Allow,
            "successes never lock"
        );
    }

    #[test]
    fn a_flood_of_new_keys_cannot_evict_a_lock() {
        let mut l = Limiter::new(LimiterConfig {
            max_keys: 100,
            ..LimiterConfig::default()
        });
        let t = Instant::now();
        for _ in 0..5 {
            l.begin("hana", ip(1), t);
        }
        for i in 0..5_000u32 {
            l.begin(
                &format!("u{i}"),
                IpAddr::V4(Ipv4Addr::from(0x0b00_0000 + i)),
                t,
            );
        }
        assert!(matches!(l.begin("hana", ip(1), t), Verdict::Locked { .. }));
    }

    #[test]
    fn ipv6_addresses_in_one_64_count_as_one() {
        let mut l = limiter();
        let t = Instant::now();
        let in_net = |n: u16| IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 2, n, n, n, n));
        for n in 0..20 {
            l.begin(&format!("v{n}"), in_net(n), t);
        }
        assert!(matches!(
            l.begin("other", in_net(999), t),
            Verdict::Locked { .. }
        ));
        let other_net = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 3, 0, 0, 0, 1));
        assert_eq!(l.begin("other", other_net, t), Verdict::Allow);
    }

    #[test]
    fn ipv4_mapped_addresses_count_as_ipv4() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            l.begin("ivy", ip(7), t);
        }
        let mapped = IpAddr::V6(Ipv4Addr::new(10, 0, 0, 7).to_ipv6_mapped());
        assert!(matches!(l.begin("ivy", mapped, t), Verdict::Locked { .. }));
    }

    #[test]
    fn a_flood_of_locked_keys_is_capped_and_new_keys_wait() {
        // Locked entries are kept, so they can't be freed by a flood; instead each kind holds at
        // most four times max_keys, and a new key past that waits until old ones leave the window.
        let mut l = Limiter::new(LimiterConfig {
            max_keys: 10,
            ..LimiterConfig::default()
        });
        let t = Instant::now();
        for i in 0..100u32 {
            let addr = IpAddr::V4(Ipv4Addr::from(0x0c00_0000 + i));
            for _ in 0..5 {
                l.begin(&format!("p{i}"), addr, t);
            }
        }
        assert!(l.key_count() <= 3 * 40, "{}", l.key_count());
        let newcomer = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        assert!(matches!(
            l.begin("newcomer", newcomer, t),
            Verdict::SlowDown { .. }
        ));
        let later = t + Duration::from_secs(15 * 60);
        assert_eq!(l.begin("newcomer", newcomer, later), Verdict::Allow);
    }

    #[test]
    fn a_new_pair_still_locks_when_the_map_is_crowded() {
        // More locked pairs than max_keys: a new pair must still count up to its limit, not be
        // dropped and start afresh on every attempt.
        let mut l = Limiter::new(LimiterConfig {
            max_keys: 10,
            ..LimiterConfig::default()
        });
        let t = Instant::now();
        for i in 0..15u32 {
            let addr = IpAddr::V4(Ipv4Addr::from(0x0d00_0000 + i));
            for _ in 0..5 {
                l.begin(&format!("q{i}"), addr, t);
            }
        }
        let addr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        for _ in 0..5 {
            assert_eq!(l.begin("target", addr, t), Verdict::Allow);
        }
        assert!(matches!(l.begin("target", addr, t), Verdict::Locked { .. }));
    }

    #[test]
    fn the_key_count_stays_bounded() {
        let mut l = Limiter::new(LimiterConfig {
            max_keys: 100,
            ..LimiterConfig::default()
        });
        let t = Instant::now();
        for i in 0..10_000u32 {
            l.begin(&format!("u{i}"), IpAddr::V4(Ipv4Addr::from(i)), t);
        }
        assert!(l.key_count() <= 3 * 4 * 100, "{}", l.key_count());
    }
}
