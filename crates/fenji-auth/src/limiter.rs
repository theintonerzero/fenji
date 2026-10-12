//! The sign-in limiter (M3 spec D12). Failures are counted in a sliding window per (username,
//! address) pair and per address, each of which locks for a while once it reaches its limit, and
//! per username, which only slows down to one attempt every `slow_interval`: so nobody can lock a
//! player out by knowing their username. It lives in memory, and a restart forgets it, which suits
//! a small server.
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LimiterConfig {
    pub pair_max: usize,
    pub addr_max: usize,
    pub user_max: usize,
    pub window: Duration,
    pub lock: Duration,
    pub slow_interval: Duration,
    /// Keys kept per kind before the oldest are dropped.
    pub max_keys: usize,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            pair_max: 5,
            addr_max: 20,
            user_max: 10,
            window: Duration::from_secs(15 * 60),
            lock: Duration::from_secs(15 * 60),
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
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
    last_attempt: Option<Instant>,
}

impl Entry {
    fn trim(&mut self, now: Instant, window: Duration) {
        while self
            .failures
            .front()
            .is_some_and(|t| now.duration_since(*t) >= window)
        {
            self.failures.pop_front();
        }
    }

    fn stale(&self, now: Instant, window: Duration) -> bool {
        self.failures
            .back()
            .is_none_or(|t| now.duration_since(*t) >= window)
            && self.locked_until.is_none_or(|u| u <= now)
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

    /// Whether an attempt may go ahead now. An attempt allowed for a slowed-down username counts
    /// as made, so the next one waits `slow_interval`.
    pub fn check(&mut self, username: &str, addr: IpAddr, now: Instant) -> Verdict {
        let user = username.to_ascii_lowercase();
        let locks = [
            self.pairs
                .get(&(user.clone(), addr))
                .and_then(|e| e.locked_until),
            self.addrs.get(&addr).and_then(|e| e.locked_until),
        ];
        if let Some(until) = locks.into_iter().flatten().filter(|u| *u > now).max() {
            return Verdict::Locked {
                retry_after: until - now,
            };
        }
        let (window, max, interval) = (self.cfg.window, self.cfg.user_max, self.cfg.slow_interval);
        if let Some(e) = self.users.get_mut(&user) {
            e.trim(now, window);
            if e.failures.len() >= max {
                if let Some(since) = e.last_attempt.map(|last| now.duration_since(last))
                    && since < interval
                {
                    return Verdict::SlowDown {
                        retry_after: interval - since,
                    };
                }
                e.last_attempt = Some(now);
            }
        }
        Verdict::Allow
    }

    pub fn record_failure(&mut self, username: &str, addr: IpAddr, now: Instant) {
        let user = username.to_ascii_lowercase();
        let cfg = self.cfg.clone();
        bump(
            self.pairs.entry((user.clone(), addr)).or_default(),
            now,
            &cfg,
            Some(cfg.pair_max),
        );
        bump(
            self.addrs.entry(addr).or_default(),
            now,
            &cfg,
            Some(cfg.addr_max),
        );
        let u = self.users.entry(user).or_default();
        bump(u, now, &cfg, None);
        u.last_attempt = Some(now);
        self.prune(now);
    }

    pub fn record_success(&mut self, username: &str, addr: IpAddr) {
        self.pairs.remove(&(username.to_ascii_lowercase(), addr));
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

/// Counts one failure; with a `lock_at`, reaching it locks the key and starts the count afresh.
fn bump(e: &mut Entry, now: Instant, cfg: &LimiterConfig, lock_at: Option<usize>) {
    e.trim(now, cfg.window);
    e.failures.push_back(now);
    if lock_at.is_some_and(|max| e.failures.len() >= max) {
        e.locked_until = Some(now + cfg.lock);
        e.failures.clear();
    }
}

fn prune_map<K: Clone + Eq + Hash>(
    map: &mut HashMap<K, Entry>,
    now: Instant,
    window: Duration,
    max: usize,
) {
    if map.len() <= max {
        return;
    }
    map.retain(|_, e| !e.stale(now, window));
    if map.len() > max {
        // A flood of fresh keys: keep the newest `max`, so memory stays bounded.
        let mut keys: Vec<(K, Instant)> = map
            .iter()
            .map(|(k, e)| (k.clone(), e.failures.back().copied().unwrap_or(now)))
            .collect();
        keys.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        for (k, _) in keys.into_iter().skip(max) {
            map.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
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
            assert_eq!(l.check("alice", ip(1), t), Verdict::Allow);
            l.record_failure("alice", ip(1), t);
        }
        assert!(matches!(l.check("alice", ip(1), t), Verdict::Locked { .. }));
        assert_eq!(l.check("alice", ip(2), t), Verdict::Allow);
    }

    #[test]
    fn the_lock_ends_and_success_clears_the_pair() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            l.record_failure("bob", ip(1), t);
        }
        let later = t + Duration::from_secs(15 * 60 + 1);
        assert_eq!(l.check("bob", ip(1), later), Verdict::Allow);
        for _ in 0..4 {
            l.record_failure("bob", ip(1), later);
        }
        l.record_success("bob", ip(1));
        l.record_failure("bob", ip(1), later);
        assert_eq!(l.check("bob", ip(1), later), Verdict::Allow);
    }

    #[test]
    fn twenty_failures_from_one_address_lock_the_address() {
        let mut l = limiter();
        let t = Instant::now();
        for i in 0..20 {
            l.record_failure(&format!("user{i}"), ip(9), t);
        }
        assert!(matches!(
            l.check("someone-else", ip(9), t),
            Verdict::Locked { .. }
        ));
        assert_eq!(l.check("someone-else", ip(8), t), Verdict::Allow);
    }

    #[test]
    fn many_failures_for_one_username_only_slow_it_down() {
        let mut l = limiter();
        let t = Instant::now();
        for i in 0..10 {
            l.record_failure("carol", ip(i), t);
        }
        // From a fresh address: never locked, but spaced 2 s apart.
        assert!(matches!(
            l.check("carol", ip(100), t),
            Verdict::SlowDown { .. }
        ));
        assert_eq!(
            l.check("carol", ip(100), t + Duration::from_secs(2)),
            Verdict::Allow
        );
        assert!(matches!(
            l.check("carol", ip(101), t + Duration::from_secs(3)),
            Verdict::SlowDown { .. }
        ));
    }

    #[test]
    fn usernames_compare_case_insensitively() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..5 {
            l.record_failure("Dave", ip(1), t);
        }
        assert!(matches!(l.check("dave", ip(1), t), Verdict::Locked { .. }));
    }

    #[test]
    fn old_failures_leave_the_window() {
        let mut l = limiter();
        let t = Instant::now();
        for _ in 0..4 {
            l.record_failure("erin", ip(1), t);
        }
        let later = t + Duration::from_secs(15 * 60);
        l.record_failure("erin", ip(1), later);
        assert_eq!(l.check("erin", ip(1), later), Verdict::Allow);
    }

    #[test]
    fn the_key_count_stays_bounded() {
        let mut l = Limiter::new(LimiterConfig {
            max_keys: 100,
            ..LimiterConfig::default()
        });
        let t = Instant::now();
        for i in 0..10_000u32 {
            l.record_failure(&format!("u{i}"), IpAddr::V4(Ipv4Addr::from(i)), t);
        }
        assert!(l.key_count() <= 3 * 100);
    }
}
