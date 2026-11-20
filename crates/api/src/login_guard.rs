//! Brute-force protection for sign-in: after [`MAX_FAILURES`] failed attempts for one
//! account (collection + identity) within [`WINDOW`], that account is refused for
//! [`LOCKOUT`], whether or not the next password is right.
//!
//! In memory and per process: with several nodes each keeps its own count, so the
//! effective limit is `nodes × MAX_FAILURES`. Put a shared limiter (reverse proxy, WAF)
//! in front for a hard guarantee. Keyed by *account*, not IP, so one attacker cannot
//! lock everyone out by hitting many accounts, and a botnet cannot dodge it by rotating IPs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_FAILURES: u32 = 5;
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
pub const LOCKOUT: Duration = Duration::from_secs(5 * 60);
const MAX_TRACKED: usize = 10_000;

#[derive(Default, Debug)]
pub struct LoginGuard {
    accounts: Mutex<HashMap<String, Entry>>,
}

#[derive(Debug)]
struct Entry {
    failures: u32,
    first: Instant,
    locked_until: Option<Instant>,
}

pub fn key(collection: &str, identity: &str) -> String {
    format!("{collection}\u{0}{}", identity.trim().to_lowercase())
}

impl LoginGuard {
    /// `Err(seconds)` while the account is locked.
    pub fn check(&self, key: &str) -> Result<(), u64> {
        self.check_at(key, Instant::now())
    }

    pub fn failure(&self, key: &str) {
        self.failure_at(key, Instant::now());
    }

    pub fn success(&self, key: &str) {
        if let Ok(mut m) = self.accounts.lock() {
            m.remove(key);
        }
    }

    fn check_at(&self, key: &str, now: Instant) -> Result<(), u64> {
        let Ok(m) = self.accounts.lock() else {
            return Ok(());
        };
        match m.get(key).and_then(|e| e.locked_until) {
            Some(until) if until > now => Err((until - now).as_secs().max(1)),
            _ => Ok(()),
        }
    }

    fn failure_at(&self, key: &str, now: Instant) {
        let Ok(mut m) = self.accounts.lock() else {
            return;
        };
        if m.len() >= MAX_TRACKED {
            m.retain(|_, e| {
                e.locked_until.is_some_and(|u| u > now) || now.duration_since(e.first) < WINDOW
            });
        }
        let e = m.entry(key.to_string()).or_insert(Entry {
            failures: 0,
            first: now,
            locked_until: None,
        });
        if now.duration_since(e.first) >= WINDOW || e.locked_until.is_some_and(|u| u <= now) {
            *e = Entry {
                failures: 0,
                first: now,
                locked_until: None,
            };
        }
        e.failures += 1;
        if e.failures >= MAX_FAILURES {
            e.locked_until = Some(now + LOCKOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_the_limit_and_unlocks_later() {
        let g = LoginGuard::default();
        let k = key("users", " Ann@X.test ");
        assert_eq!(
            k,
            key("users", "ann@x.test"),
            "case and whitespace do not dodge it"
        );
        let t0 = Instant::now();
        for _ in 0..MAX_FAILURES - 1 {
            g.failure_at(&k, t0);
            assert!(g.check_at(&k, t0).is_ok());
        }
        g.failure_at(&k, t0);
        let wait = g.check_at(&k, t0).unwrap_err();
        assert!((1..=LOCKOUT.as_secs()).contains(&wait));
        assert!(
            g.check_at(&key("users", "other@x.test"), t0).is_ok(),
            "other accounts are unaffected"
        );
        assert!(
            g.check_at(&key("admins", "ann@x.test"), t0).is_ok(),
            "keyed per collection"
        );
        assert!(
            g.check_at(&k, t0 + LOCKOUT + Duration::from_secs(1))
                .is_ok()
        );
    }

    #[test]
    fn success_resets_and_old_failures_expire() {
        let g = LoginGuard::default();
        let k = key("users", "a@b.co");
        let t0 = Instant::now();
        for _ in 0..MAX_FAILURES - 1 {
            g.failure_at(&k, t0);
        }
        g.success(&k);
        g.failure_at(&k, t0);
        assert!(
            g.check_at(&k, t0).is_ok(),
            "the count restarted after a success"
        );
        for _ in 0..MAX_FAILURES - 1 {
            g.failure_at(&k, t0 + WINDOW + Duration::from_secs(1));
        }
        assert!(
            g.check_at(&k, t0 + WINDOW + Duration::from_secs(1)).is_ok(),
            "failures outside the window do not add up"
        );
    }
}
