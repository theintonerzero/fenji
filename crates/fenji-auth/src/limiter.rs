//! The sign-in limiter (M3 spec D12). Attempts are counted in a sliding window per (username,
//! address) pair and per address, either of which refuses further attempts while it holds its
//! limit, and per username, which only slows down to one attempt every `slow_interval`: so nobody
//! can lock a player out by knowing their username.
//!
//! An attempt counts the moment it begins, and a success takes it back, so attempts made in
//! parallel are counted before their password checks finish. Addresses count by network: an IPv4
//! address alone, an IPv6 address by its /64, which one subscriber usually holds whole.
//!
//! The limiter never asks whether an account exists, so its answers can't tell. A username's
//! slow-down counts in one of [`NAME_BUCKETS`] buckets, chosen by a hash keyed with a secret drawn
//! per process: every name is treated alike, the table stays bounded, and nobody can aim names at
//! another's bucket. A bucket's few other names share its slow-down, which is the price of the
//! bound.
//!
//! Memory and work stay bounded without a global lockout: stale entries are dropped at most once
//! per `prune_every`; an entry with an attempt in the window is never dropped; and past four times
//! `max_keys` a new pair or network simply isn't counted on its own. The name's slow-down still
//! applies, so a flood of networks can neither lock newcomers out nor lift anyone's limit. It
//! lives in memory, and a restart forgets it, which suits a small server.
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::net::{IpAddr, Ipv6Addr};
use std::time::{Duration, Instant};

/// How many buckets the usernames' slow-downs share.
pub const NAME_BUCKETS: u64 = 4096;

#[derive(Debug, Clone)]
pub struct LimiterConfig {
    pub pair_max: usize,
    pub addr_max: usize,
    pub name_max: usize,
    pub window: Duration,
    pub slow_interval: Duration,
    /// Keys per kind past which stale entries are dropped; past four times this, new pairs and
    /// networks aren't counted on their own.
    pub max_keys: usize,
    /// The least time between two sweeps for stale entries.
    pub prune_every: Duration,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            pair_max: 5,
            addr_max: 20,
            name_max: 10,
            window: Duration::from_secs(15 * 60),
            slow_interval: Duration::from_secs(2),
            max_keys: 10_000,
            prune_every: Duration::from_secs(1),
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
    fn locked_for(&mut self, now: Instant, window: Duration, max: usize) -> Option<Duration> {
        self.trim(now, window);
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

/// The hash that picks a name's bucket.
enum BucketKey {
    /// SipHash with a key drawn per process.
    Secret(RandomState),
    /// A fixed key, so tests are repeatable.
    #[cfg(test)]
    Fixed,
}

pub struct Limiter {
    cfg: LimiterConfig,
    pairs: HashMap<(String, IpAddr), Entry>,
    addrs: HashMap<IpAddr, Entry>,
    names: HashMap<u64, Entry>,
    last_prune: Option<Instant>,
    sweeps: u64,
    key: BucketKey,
}

impl Limiter {
    pub fn new(cfg: LimiterConfig) -> Self {
        Self::with_key(cfg, BucketKey::Secret(RandomState::new()))
    }

    #[cfg(test)]
    fn fixed(cfg: LimiterConfig) -> Self {
        Self::with_key(cfg, BucketKey::Fixed)
    }

    fn with_key(cfg: LimiterConfig, key: BucketKey) -> Self {
        Self {
            cfg,
            pairs: HashMap::new(),
            addrs: HashMap::new(),
            names: HashMap::new(),
            last_prune: None,
            sweeps: 0,
            key,
        }
    }

    fn bucket(&self, lower: &str) -> u64 {
        let h = match &self.key {
            BucketKey::Secret(state) => state.hash_one(lower),
            #[cfg(test)]
            BucketKey::Fixed => {
                use std::hash::{DefaultHasher, Hasher};
                let mut h = DefaultHasher::new();
                lower.hash(&mut h);
                h.finish()
            }
        };
        h % NAME_BUCKETS
    }

    /// Starts an attempt to sign in as `username` from `addr`. It is refused while its pair or its
    /// address holds its limit, or while its name's bucket is slowed down. An allowed attempt
    /// counts as a failure until [`Limiter::succeed`] takes it back.
    pub fn begin(&mut self, username: &str, addr: IpAddr, now: Instant) -> Verdict {
        self.prune(now);
        let user = username.to_ascii_lowercase();
        let (bucket, net) = (self.bucket(&user), net_key(addr));
        let cfg = &self.cfg;
        let pair = (user, net);
        let pair_lock = self
            .pairs
            .get_mut(&pair)
            .and_then(|e| e.locked_for(now, cfg.window, cfg.pair_max));
        let addr_lock = self
            .addrs
            .get_mut(&net)
            .and_then(|e| e.locked_for(now, cfg.window, cfg.addr_max));
        if let Some(retry_after) = pair_lock.into_iter().chain(addr_lock).max() {
            return Verdict::Locked { retry_after };
        }
        let n = self.names.entry(bucket).or_default();
        n.trim(now, cfg.window);
        if n.attempts.len() >= cfg.name_max
            && let Some(since) = n.last_attempt.map(|last| now.duration_since(last))
            && since < cfg.slow_interval
        {
            return Verdict::SlowDown {
                retry_after: cfg.slow_interval - since,
            };
        }
        n.attempts.push_back(now);
        n.last_attempt = Some(now);
        let hard = cfg.max_keys.saturating_mul(4);
        count(&mut self.pairs, pair, now, hard);
        count(&mut self.addrs, net, now, hard);
        Verdict::Allow
    }

    /// The attempt to sign in as `username` succeeded: its pair starts afresh, and its address and
    /// name get the attempt back.
    pub fn succeed(&mut self, username: &str, addr: IpAddr) {
        let user = username.to_ascii_lowercase();
        let (bucket, net) = (self.bucket(&user), net_key(addr));
        self.pairs.remove(&(user, net));
        if let Some(a) = self.addrs.get_mut(&net) {
            a.attempts.pop_back();
        }
        if let Some(n) = self.names.get_mut(&bucket) {
            n.attempts.pop_back();
        }
    }

    /// Entries held: pairs, networks and name buckets (for tests).
    pub fn sizes(&self) -> (usize, usize, usize) {
        (self.pairs.len(), self.addrs.len(), self.names.len())
    }

    /// How many sweeps for stale entries have run (for tests).
    pub fn sweeps(&self) -> u64 {
        self.sweeps
    }

    /// Past `max_keys` pairs or networks, and at most once per `prune_every`: drops the entries
    /// whose attempts have all left the window. An entry with an attempt in the window is never
    /// dropped, since that would forget its count.
    fn prune(&mut self, now: Instant) {
        let max = self.cfg.max_keys;
        if self.pairs.len() <= max && self.addrs.len() <= max {
            return;
        }
        if self
            .last_prune
            .is_some_and(|last| now.duration_since(last) < self.cfg.prune_every)
        {
            return;
        }
        self.last_prune = Some(now);
        self.sweeps += 1;
        let window = self.cfg.window;
        drop_stale(&mut self.pairs, now, window);
        drop_stale(&mut self.addrs, now, window);
        drop_stale(&mut self.names, now, window);
    }
}

/// Counts an attempt for `key`; a key not yet held past `hard` keys isn't counted on its own.
fn count<K: Eq + Hash>(map: &mut HashMap<K, Entry>, key: K, now: Instant, hard: usize) {
    if let Some(e) = map.get_mut(&key) {
        e.attempts.push_back(now);
    } else if map.len() < hard {
        map.entry(key).or_default().attempts.push_back(now);
    }
}

fn drop_stale<K: Eq + Hash>(map: &mut HashMap<K, Entry>, now: Instant, window: Duration) {
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

    fn net(i: u32) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from(0x0c00_0000 + i))
    }

    fn limiter() -> Limiter {
        Limiter::fixed(LimiterConfig::default())
    }

    fn small() -> Limiter {
        Limiter::fixed(LimiterConfig {
            max_keys: 10,
            ..LimiterConfig::default()
        })
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
        for _ in 0..5 {
            assert_eq!(l.begin("bob", ip(1), later), Verdict::Allow);
        }
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
            l.begin("someone", ip(9), t),
            Verdict::Locked { .. }
        ));
        assert_eq!(l.begin("someone", ip(8), t), Verdict::Allow);
    }

    #[test]
    fn many_failures_for_one_account_only_slow_it_down() {
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
    fn accounts_compare_case_insensitively() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            l.begin("Dave", ip(1), t);
        }
        assert!(matches!(l.begin("dave", ip(1), t), Verdict::Locked { .. }));
    }

    #[test]
    fn the_slow_down_never_asks_whether_an_account_exists() {
        // Every username counts in its bucket, so a name's failures slow down that name (and
        // the few names sharing its bucket), the same way whether it has an account or not.
        let mut l = limiter();
        let t = Instant::now();
        for i in 0..10 {
            l.begin("ghost", ip(i), t);
        }
        assert!(matches!(
            l.begin("ghost", ip(100), t),
            Verdict::SlowDown { .. }
        ));
        let others = ["casper", "wraith", "shade", "spook", "phantom"];
        assert!(
            others
                .iter()
                .any(|n| l.begin(n, ip(101), t) == Verdict::Allow),
            "other names have buckets of their own"
        );
    }

    #[test]
    fn name_buckets_are_bounded() {
        let mut l = small();
        let t = Instant::now();
        for i in 0..20_000u32 {
            l.begin(&format!("nobody{i}"), net(i % 30), t);
        }
        let (_, _, buckets) = l.sizes();
        assert!(buckets <= NAME_BUCKETS as usize, "{buckets}");
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
    fn a_flood_of_networks_cannot_evict_a_lock() {
        let mut l = small();
        let t = Instant::now();
        for _ in 0..5 {
            l.begin("hana", ip(1), t);
        }
        for i in 0..5_000u32 {
            l.begin(&format!("f{i}"), net(i), t);
        }
        assert!(matches!(l.begin("hana", ip(1), t), Verdict::Locked { .. }));
    }

    #[test]
    fn a_flood_of_networks_cannot_lock_out_newcomers() {
        let mut l = small();
        let t = Instant::now();
        for i in 0..100u32 {
            for _ in 0..5 {
                l.begin(&format!("f{i}"), net(i), t);
            }
        }
        let (pairs, addrs, _) = l.sizes();
        assert_eq!(
            (pairs, addrs),
            (40, 40),
            "both tables at four times max_keys"
        );
        let newcomer = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        assert_eq!(l.begin("newbie", newcomer, t), Verdict::Allow);
    }

    #[test]
    fn a_flood_cannot_lift_an_accounts_slowdown() {
        let mut l = small();
        let t = Instant::now();
        for i in 0..10 {
            l.begin("target", ip(i), t);
        }
        for i in 0..1_000u32 {
            l.begin(&format!("f{i}"), net(i), t);
        }
        let fresh = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));
        assert!(matches!(
            l.begin("target", fresh, t),
            Verdict::SlowDown { .. }
        ));
    }

    #[test]
    fn a_new_pair_still_locks_when_the_map_is_crowded() {
        // More locked pairs than max_keys: a new pair must still count up to its limit.
        let mut l = small();
        let t = Instant::now();
        for i in 0..15u32 {
            for _ in 0..5 {
                l.begin(&format!("q{i}"), net(i), t);
            }
        }
        let addr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        for _ in 0..5 {
            assert_eq!(l.begin("target", addr, t), Verdict::Allow);
        }
        assert!(matches!(l.begin("target", addr, t), Verdict::Locked { .. }));
    }

    #[test]
    fn memory_stays_bounded() {
        let mut l = Limiter::fixed(LimiterConfig {
            max_keys: 100,
            ..LimiterConfig::default()
        });
        let t = Instant::now();
        for i in 0..10_000u32 {
            l.begin(&format!("f{i}"), IpAddr::V4(Ipv4Addr::from(i)), t);
        }
        let (pairs, addrs, _) = l.sizes();
        assert!(pairs <= 400 && addrs <= 400, "{pairs} {addrs}");
    }

    #[test]
    fn sweeps_run_at_most_once_per_interval() {
        let mut l = small();
        let t = Instant::now();
        for i in 0..1_000u32 {
            l.begin(&format!("f{i}"), net(i), t);
        }
        assert_eq!(
            l.sweeps(),
            1,
            "one sweep however many attempts arrive at once"
        );
        l.begin("late", net(5_000), t + Duration::from_secs(1));
        assert_eq!(l.sweeps(), 2);
    }
}
