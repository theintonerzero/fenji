//! Time: the clock, injectable so tests can expire tokens without waiting, and its moments as dates
//! in UTC.
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// A clock tests move by hand.
#[derive(Clone, Default)]
pub struct TestClock(Arc<AtomicI64>);

impl TestClock {
    pub fn at(ms: i64) -> Self {
        Self(Arc::new(AtomicI64::new(ms)))
    }

    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A moment as a date and a time of day in UTC, to the second: what the CLI shows, and what
/// backups are named for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
}

impl Utc {
    /// The moment `ms` Unix milliseconds name, the part second dropped.
    pub fn from_ms(ms: i64) -> Self {
        let secs = ms.div_euclid(1000);
        let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        Self {
            year: yoe + era * 400 + i64::from(month <= 2),
            month,
            day: doy - (153 * mp + 2) / 5 + 1,
            hour: rem / 3600,
            minute: rem % 3600 / 60,
            second: rem % 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Utc;

    #[test]
    fn milliseconds_name_a_date_and_time_in_utc() {
        let at = |year, month, day, hour, minute, second| Utc {
            year,
            month,
            day,
            hour,
            minute,
            second,
        };
        assert_eq!(Utc::from_ms(0), at(1970, 1, 1, 0, 0, 0));
        assert_eq!(Utc::from_ms(1_791_950_400_000), at(2026, 10, 14, 4, 0, 0));
        // A leap day's last millisecond: the part second is dropped.
        assert_eq!(Utc::from_ms(1_709_251_199_999), at(2024, 2, 29, 23, 59, 59));
        // Before 1970 too.
        assert_eq!(Utc::from_ms(-1), at(1969, 12, 31, 23, 59, 59));
    }
}
