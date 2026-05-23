//! Simple in-memory rate limiter for the HTTP server.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
#[cfg(test)]
const MAX_REQUESTS: u32 = 10_000;
#[cfg(not(test))]
const MAX_REQUESTS: u32 = 60;

#[derive(Default)]
struct RateLimiter {
    buckets: HashMap<IpAddr, (u32, Instant)>,
}

impl RateLimiter {
    fn check(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let entry = self.buckets.entry(ip).or_insert((0, now));
        if now.duration_since(entry.1) >= WINDOW {
            *entry = (0, now);
        }
        if entry.0 >= MAX_REQUESTS {
            return false;
        }
        entry.0 += 1;
        true
    }
}

fn limiter() -> &'static Mutex<RateLimiter> {
    static LIMITER: OnceLock<Mutex<RateLimiter>> = OnceLock::new();
    LIMITER.get_or_init(|| Mutex::new(RateLimiter::default()))
}

/// Returns false when the client exceeded the per-IP request budget.
pub fn allow(ip: IpAddr) -> bool {
    limiter().lock().expect("rate limiter lock").check(ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn allows_requests_under_limit() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        for _ in 0..10 {
            assert!(allow(ip));
        }
    }
}
