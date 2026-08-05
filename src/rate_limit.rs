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
/// On mutex poison, fail closed (deny) rather than panic.
pub fn allow(ip: IpAddr) -> bool {
    let Ok(mut guard) = limiter().lock() else {
        return false;
    };
    guard.check(ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn allows_requests_under_limit() {
        // Unique TEST-NET IP so parallel tests don't share a bucket.
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        for _ in 0..10 {
            assert!(allow(ip));
        }
    }

    #[test]
    fn local_check_allows_then_denies_at_max() {
        let mut rl = RateLimiter::default();
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 50));
        for i in 0..MAX_REQUESTS {
            assert!(rl.check(ip), "request {i} should be allowed");
        }
        assert!(!rl.check(ip), "request past MAX_REQUESTS must be denied");
        assert!(!rl.check(ip), "subsequent requests stay denied in-window");
    }

    #[test]
    fn local_check_independent_ips() {
        let mut rl = RateLimiter::default();
        let a = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        let b = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 2));
        for _ in 0..MAX_REQUESTS {
            assert!(rl.check(a));
        }
        assert!(!rl.check(a));
        // Exhausting A must not affect B
        assert!(rl.check(b));
    }

    #[test]
    fn local_check_supports_ipv6() {
        let mut rl = RateLimiter::default();
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        assert!(rl.check(ip));
        assert!(rl.check(ip));
    }

    #[test]
    fn allow_public_api_uses_unique_ip() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 99));
        assert!(allow(ip));
        assert!(allow(ip));
    }

    #[test]
    fn default_rate_limiter_starts_empty() {
        let rl = RateLimiter::default();
        assert!(rl.buckets.is_empty());
    }
}
