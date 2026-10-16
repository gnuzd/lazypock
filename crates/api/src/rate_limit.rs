//! Per-node request rate limiting.
//!
//! PLAN.md section 8.4 and line 447: rate limits are **in memory, per node**, with
//! shared exact limits across nodes named as a trigger for a Redis sidecar rather
//! than a requirement. So this is deliberately not shared — two nodes behind a load
//! balancer allow twice the limit, and that is the documented trade rather than an
//! oversight.
//!
//! The key is the client's address, from [`crate::client::address`]. A limiter
//! keyed on an address the caller chooses is not a limiter, which is why the
//! trusted-proxy rule had to come first.
//!
//! Two allowances rather than one: a request that spends credentials gets a much
//! smaller budget than an ordinary read. A single limit either lets a password be
//! guessed for as long as ordinary traffic is allowed, or refuses ordinary traffic
//! as fast as it refuses guesses.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::state::AppState;

/// How many distinct keys are held before idle ones are swept.
///
/// A key per address is bounded in practice, but "in practice" is how a limiter
/// becomes a slow leak on a deployment that sees many addresses and never restarts.
const SWEEP_AT: usize = 10_000;

/// How long an account's soft lockout lasts, from its first failure in the window.
///
/// Long enough that guessing is not worth it, short enough that someone who
/// fumbled a password can come back without an administrator. It expires on its
/// own, which is what makes it *soft*: there is no state to clear.
pub const ACCOUNT_LOCKOUT_WINDOW: Duration = Duration::from_secs(15 * 60);

/// A fixed-window counter per key.
pub struct Limiter {
    /// Requests allowed per window. Zero disables this limiter entirely.
    limit: u32,
    window: Duration,
    state: Mutex<Buckets>,
}

struct Buckets {
    counts: HashMap<String, Count>,
    /// When the map was last swept.
    swept: Instant,
}

struct Count {
    used: u32,
    started: Instant,
}

/// Why a request was refused.
pub struct Refused {
    /// How long until the window resets.
    pub retry_after: Duration,
}

impl Limiter {
    /// A limiter allowing `limit` requests per `window`.
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            state: Mutex::new(Buckets {
                counts: HashMap::new(),
                swept: Instant::now(),
            }),
        }
    }

    /// Whether this limiter does anything.
    pub fn is_enabled(&self) -> bool {
        self.limit > 0
    }

    /// Count a request against `key`, or refuse it.
    pub fn check(&self, key: &str) -> Result<(), Refused> {
        if !self.is_enabled() {
            return Ok(());
        }

        let now = Instant::now();
        // A panic elsewhere must not disable limiting, so a poisoned lock is taken
        // rather than propagated: the counters are still usable.
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        state.sweep(now, self.window);

        let count = state.counts.entry(key.to_string()).or_insert(Count {
            used: 0,
            started: now,
        });

        // A fixed window: the count resets when the window it started in is over.
        if count.started.elapsed() >= self.window {
            count.used = 0;
            count.started = now;
        }

        if count.used >= self.limit {
            return Err(Refused {
                retry_after: self.window.saturating_sub(count.started.elapsed()),
            });
        }

        count.used += 1;
        Ok(())
    }
}

impl Buckets {
    /// Drop keys whose window has long passed, once there are enough to be worth
    /// walking. `retain` over a map this size is not something to do per request.
    fn sweep(&mut self, now: Instant, window: Duration) {
        if self.counts.len() < SWEEP_AT || now.duration_since(self.swept) < window {
            return;
        }
        self.counts
            .retain(|_, count| count.started.elapsed() < window);
        self.swept = now;
    }
}

/// A fixed-window count of **failed** credential attempts, keyed by account.
///
/// The per-address [`Limiter`] counts every credential request. This counts only
/// failures, and is consulted before each attempt. Counting failures rather than
/// requests is what stops an attacker who knows a victim's address from spending
/// the victim's budget and locking the real user out of their own sign-in. A
/// successful sign-in clears the count, so fumbles do not accumulate across
/// sessions.
///
/// The account is the collection plus the submitted identity, so the same address
/// in two auth collections is two accounts, and a non-existent account is counted
/// the same as a real one (which is what stops it being an enumeration oracle).
pub struct Lockout {
    limit: u32,
    window: Duration,
    state: Mutex<Buckets>,
}

impl Lockout {
    /// A lockout after `limit` failures in `window`. Zero disables it.
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            state: Mutex::new(Buckets {
                counts: HashMap::new(),
                swept: Instant::now(),
            }),
        }
    }

    /// Whether this lockout does anything.
    pub fn is_enabled(&self) -> bool {
        self.limit > 0
    }

    /// Whether `key` has exhausted its failures, and how long until it recovers.
    pub fn is_locked(&self, key: &str) -> Result<(), Refused> {
        if !self.is_enabled() {
            return Ok(());
        }

        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sweep(now, self.window);

        match state.counts.get(key) {
            Some(count) if count.used >= self.limit && count.started.elapsed() < self.window => {
                Err(Refused {
                    retry_after: self.window.saturating_sub(count.started.elapsed()),
                })
            }
            _ => Ok(()),
        }
    }

    /// Count one failed attempt.
    pub fn record_failure(&self, key: &str) {
        if !self.is_enabled() {
            return;
        }

        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sweep(now, self.window);

        let count = state.counts.entry(key.to_string()).or_insert(Count {
            used: 0,
            started: now,
        });
        if count.started.elapsed() >= self.window {
            count.used = 0;
            count.started = now;
        }
        count.used += 1;
    }

    /// Forget a key's failures, after a successful sign-in.
    pub fn clear(&self, key: &str) {
        if !self.is_enabled() {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.counts.remove(key);
    }
}

/// The allowances a node enforces.
pub struct Limits {
    /// Requests that spend credentials: signing in, and refreshing.
    pub credentials: Limiter,
    /// Everything else.
    pub requests: Limiter,
    /// Failed sign-ins per account, over [`ACCOUNT_LOCKOUT_WINDOW`].
    pub account: Lockout,
}

impl Limits {
    /// Build from the configured allowances.
    pub fn new(
        credentials_per_minute: u32,
        requests_per_minute: u32,
        account_failures: u32,
    ) -> Self {
        Self {
            credentials: Limiter::new(credentials_per_minute, Duration::from_secs(60)),
            requests: Limiter::new(requests_per_minute, Duration::from_secs(60)),
            account: Lockout::new(account_failures, ACCOUNT_LOCKOUT_WINDOW),
        }
    }
}

/// Refuse a request that has spent its allowance.
///
/// Sits inside the request logger, so a refusal is recorded, and inside CORS, so a
/// preflight is answered without spending anything.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();

    // The health probe is exempt, for the same reason the logger exempts it: a
    // load balancer polls it on its own schedule, and refusing one would take the
    // node out of rotation for being polled.
    if path == "/api/health" {
        return next.run(request).await;
    }

    let limiter = if spends_credentials(&path) {
        &state.rate_limits.credentials
    } else {
        &state.rate_limits.requests
    };

    if !limiter.is_enabled() {
        return next.run(request).await;
    }

    let peer = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let key = crate::client::address(&state.config, peer, request.headers())
        .map(|ip| ip.to_string())
        // An in-process request has no address. One shared bucket is the honest
        // answer: there is nothing to tell those requests apart by.
        .unwrap_or_else(|| "unknown".to_string());

    if let Err(refused) = limiter.check(&key) {
        let seconds = refused.retry_after.as_secs().max(1);
        tracing::warn!(key = %key, path = %path, "rate limit reached");
        return too_many(
            seconds,
            format!("Too many requests. Try again in {seconds} seconds."),
        );
    }

    next.run(request).await
}

/// The refusal response, with the wait in `Retry-After`.
///
/// Shared with the per-account lockout so both refusals have one shape.
pub(crate) fn too_many(seconds: u64, message: String) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(
            header::RETRY_AFTER,
            HeaderValue::from_str(&seconds.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("60")),
        )],
        Json(json!({ "status": 429, "message": message, "data": {} })),
    )
        .into_response()
}

/// Whether a path spends credentials, and therefore gets the smaller allowance.
fn spends_credentials(path: &str) -> bool {
    path.ends_with("/auth-with-password") || path.ends_with("/auth-refresh")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(limit: u32) -> Limiter {
        Limiter::new(limit, Duration::from_secs(60))
    }

    #[test]
    fn a_zero_limit_disables_the_limiter() {
        let limiter = limiter(0);
        assert!(!limiter.is_enabled());
        for _ in 0..1_000 {
            assert!(limiter.check("1.2.3.4").is_ok());
        }
    }

    #[test]
    fn the_allowance_is_spent_and_then_refused() {
        let limiter = limiter(3);
        for _ in 0..3 {
            assert!(limiter.check("1.2.3.4").is_ok());
        }
        assert!(limiter.check("1.2.3.4").is_err());
        // And a different key has its own allowance.
        assert!(limiter.check("5.6.7.8").is_ok());
    }

    #[test]
    fn a_refusal_says_how_long_to_wait() {
        let limiter = limiter(1);
        assert!(limiter.check("k").is_ok());
        let refused = limiter
            .check("k")
            .expect_err("the second request is refused");
        assert!(
            refused.retry_after <= Duration::from_secs(60),
            "the wait should be within the window, got {:?}",
            refused.retry_after
        );
    }

    /// The window is fixed, so a key that has waited it out starts again.
    #[test]
    fn the_allowance_returns_when_the_window_passes() {
        let limiter = Limiter::new(1, Duration::from_millis(20));
        assert!(limiter.check("k").is_ok());
        assert!(limiter.check("k").is_err());
        std::thread::sleep(Duration::from_millis(30));
        assert!(limiter.check("k").is_ok(), "the window should have reset");
    }

    #[test]
    fn only_credential_paths_get_the_smaller_allowance() {
        assert!(spends_credentials(
            "/api/collections/_superusers/auth-with-password"
        ));
        assert!(spends_credentials("/api/collections/users/auth-refresh"));
        assert!(!spends_credentials("/api/collections/posts/records"));
        assert!(!spends_credentials("/api/collections/users/auth-methods"));
        assert!(!spends_credentials("/api/health"));
    }

    #[test]
    fn an_account_locks_only_after_its_failures_and_recovers_with_the_window() {
        let lockout = Lockout::new(2, Duration::from_millis(30));
        assert!(lockout.is_locked("a@example.com").is_ok());

        lockout.record_failure("a@example.com");
        assert!(
            lockout.is_locked("a@example.com").is_ok(),
            "one failure is not a lockout"
        );

        lockout.record_failure("a@example.com");
        assert!(lockout.is_locked("a@example.com").is_err(), "two lock it");
        // A different account has its own count.
        assert!(lockout.is_locked("b@example.com").is_ok());

        std::thread::sleep(Duration::from_millis(40));
        assert!(
            lockout.is_locked("a@example.com").is_ok(),
            "the window should have passed"
        );
    }

    #[test]
    fn a_successful_sign_in_clears_the_failures() {
        let lockout = Lockout::new(2, Duration::from_secs(60));
        lockout.record_failure("a@example.com");
        lockout.record_failure("a@example.com");
        assert!(lockout.is_locked("a@example.com").is_err());

        lockout.clear("a@example.com");
        assert!(
            lockout.is_locked("a@example.com").is_ok(),
            "a success clears the record"
        );
    }

    #[test]
    fn a_zero_account_limit_disables_the_lockout() {
        let lockout = Lockout::new(0, Duration::from_secs(60));
        assert!(!lockout.is_enabled());
        for _ in 0..1_000 {
            lockout.record_failure("a@example.com");
        }
        assert!(lockout.is_locked("a@example.com").is_ok());
    }
}
