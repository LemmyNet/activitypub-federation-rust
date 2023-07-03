use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use std::ops::Sub;

pub struct InstanceRatelimit<const LIMIT: usize> {
    period: Duration,
    data: HashMap<String, RateLimiter<LIMIT>>,
}

impl<const LIMIT: usize> InstanceRatelimit<LIMIT> {
    pub fn new(period: Duration) -> Self {
        InstanceRatelimit {
            period,
            data: HashMap::new(),
        }
    }

    fn domain_limiter(&mut self, domain: &str) -> &mut RateLimiter<LIMIT> {
        // TODO: inefficient, we only need String when inserting new entry which is rare
        let domain = domain.to_string();
        self.data.entry(domain).or_insert_with(|| RateLimiter::new(self.period))
    }

    pub fn check(&mut self, domain: &str) -> bool {
        self.domain_limiter(domain).check()
    }

    pub fn log(&mut self, domain: &str) {
        self.domain_limiter(domain).log()
    }
}

// TODO: check lemmy rate limiting code
struct RateLimiter<const LIMIT: usize> {
    period: Duration,
    /// Using limit + 1 for greater than check
    /// TODO: check if this is necessary or not
    readings: [Option<Instant>; LIMIT + 1],
}

impl<const LIMIT: usize> RateLimiter<LIMIT> {
    pub fn new(period: Duration) -> RateLimiter<LIMIT> {
        RateLimiter {
            period,
            readings: [None; LIMIT + 1],
        }
    }

    /// Count amount of entries less than `period` time before now and check against limit.
    /// Return true if it is less.
    fn check(&self) -> bool {
        let now = Instant::now();
        let count = self.readings.iter()
            .filter(|r| r.is_some())
            // TODO: check if gt/lt is correct
            .filter(|r| r.unwrap() < now.sub(self.period))
            .count();
        count > LIMIT
    }

    pub fn log(&mut self) {
        let now = Instant::now();
        // TODO: replace all items older than `period` with None, insert Some(now)
    }
}

#[cfg(test)]
pub mod test {
    use std::thread::sleep;
    use std::time::Duration;
    use crate::rate_limit::RateLimiter;

    #[test]
    fn test_limiting() {
        let mut limiter = RateLimiter::<1>::new(Duration::from_secs(1));
        assert_eq!(limiter.check(), true);
        limiter.log();
        assert_eq!(limiter.check(), true);
        limiter.log();
        assert_eq!(limiter.check(), false);
    }

    #[test]
    fn test_expiration() {
        let mut limiter = RateLimiter::<1>::new(Duration::from_secs(1));
        assert_eq!(limiter.check(), true);
        limiter.log();
        assert_eq!(limiter.check(), false);
        sleep(Duration::from_secs(1));
        assert_eq!(limiter.check(), true);

    }
}