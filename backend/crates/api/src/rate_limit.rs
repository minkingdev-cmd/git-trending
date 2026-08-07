//! In-process rate limiters for discover Search (global + per-user windows).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ght_core::config::Settings;

const WINDOW_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitScope {
    Global,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitError {
    pub scope: RateLimitScope,
    pub retry_after_secs: u64,
}

/// Sliding log of request timestamps within a 60s window.
#[derive(Debug, Default)]
struct Window {
    timestamps: Vec<Instant>,
}

impl Window {
    fn prune(&mut self, window: Duration, now: Instant) {
        let cutoff = now.checked_sub(window).unwrap_or(now);
        self.timestamps.retain(|t| *t > cutoff);
    }

    fn count(&self) -> usize {
        self.timestamps.len()
    }

    /// Seconds until the oldest in-window hit leaves the window (at least 1 when limited).
    fn retry_after_secs(&self, window: Duration, now: Instant) -> u64 {
        match self.timestamps.first() {
            Some(oldest) => {
                let elapsed = now.duration_since(*oldest);
                window.saturating_sub(elapsed).as_secs().max(1)
            }
            None => 1,
        }
    }

    fn record(&mut self, now: Instant) {
        self.timestamps.push(now);
    }
}

struct Inner {
    /// Shared-path global bucket (`"global"`).
    global: Window,
    /// Shared-path per-user buckets (`"u:{id}"`).
    users: HashMap<i64, Window>,
    /// User-token path per-user buckets (`"ut:{id}"`).
    user_tokens: HashMap<i64, Window>,
}

/// Process-local discover rate limiters.
///
/// - **shared** (`check_shared`): global then per-user; both buckets consume on success.
/// - **user token** (`check_user_token`): per-user only (higher limit); does **not** touch global.
#[derive(Clone)]
pub struct DiscoverRateLimiters {
    inner: Arc<Mutex<Inner>>,
    global_limit: u32,
    per_user_limit: u32,
    per_user_with_token_limit: u32,
    window: Duration,
}

impl DiscoverRateLimiters {
    pub fn from_settings(s: &Settings) -> Self {
        Self::new(
            s.discover_rate_limit_per_min,
            s.discover_rate_limit_per_user_per_min,
            s.discover_rate_limit_per_user_with_token_per_min,
        )
    }

    pub fn new(global_limit: u32, per_user_limit: u32, per_user_with_token_limit: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                global: Window::default(),
                users: HashMap::new(),
                user_tokens: HashMap::new(),
            })),
            global_limit,
            per_user_limit,
            per_user_with_token_limit,
            window: Duration::from_secs(WINDOW_SECS),
        }
    }

    /// Shared path: check global, then per-user; on success consume both.
    /// Either limit exceeded → `Err` and **no** counters are incremented.
    pub fn check_shared(&self, user_id: i64) -> Result<(), RateLimitError> {
        let mut guard = self.inner.lock().expect("discover rate limiter mutex");
        let now = Instant::now();
        let window = self.window;

        guard.global.prune(window, now);
        if guard.global.count() >= self.global_limit as usize {
            return Err(RateLimitError {
                scope: RateLimitScope::Global,
                retry_after_secs: guard.global.retry_after_secs(window, now),
            });
        }

        {
            let user = guard.users.entry(user_id).or_default();
            user.prune(window, now);
            if user.count() >= self.per_user_limit as usize {
                return Err(RateLimitError {
                    scope: RateLimitScope::User,
                    retry_after_secs: user.retry_after_secs(window, now),
                });
            }
            user.record(now);
        }
        guard.global.record(now);
        Ok(())
    }

    /// User-token path: per-user only (higher limit). Does **not** consume the global bucket.
    pub fn check_user_token(&self, user_id: i64) -> Result<(), RateLimitError> {
        let mut guard = self.inner.lock().expect("discover rate limiter mutex");
        let now = Instant::now();
        let window = self.window;

        let user = guard.user_tokens.entry(user_id).or_default();
        user.prune(window, now);
        if user.count() >= self.per_user_with_token_limit as usize {
            return Err(RateLimitError {
                scope: RateLimitScope::User,
                retry_after_secs: user.retry_after_secs(window, now),
            });
        }
        user.record(now);
        Ok(())
    }

    /// Current global hit count in the window (test/introspection helper).
    #[cfg(test)]
    fn global_count(&self) -> usize {
        let mut guard = self.inner.lock().expect("discover rate limiter mutex");
        let now = Instant::now();
        guard.global.prune(self.window, now);
        guard.global.count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_global_allows_limit_then_rejects() {
        // High per-user so we hit global first with a single user.
        let lim = DiscoverRateLimiters::new(20, 100, 25);
        for i in 0..20 {
            lim.check_shared(1)
                .unwrap_or_else(|e| panic!("request {} should pass: {:?}", i + 1, e));
        }
        let err = lim.check_shared(1).expect_err("21st shared should fail");
        assert_eq!(err.scope, RateLimitScope::Global);
        assert!(err.retry_after_secs >= 1);
        assert_eq!(lim.global_count(), 20);
    }

    #[test]
    fn shared_per_user_rejects_before_global_when_user_limit_lower() {
        let lim = DiscoverRateLimiters::new(20, 2, 25);
        lim.check_shared(1).unwrap();
        lim.check_shared(1).unwrap();
        let err = lim.check_shared(1).expect_err("3rd for same user");
        assert_eq!(err.scope, RateLimitScope::User);
        // Global only recorded two successful requests.
        assert_eq!(lim.global_count(), 2);
        // Different user still ok (global has room).
        lim.check_shared(2).unwrap();
        assert_eq!(lim.global_count(), 3);
    }

    #[test]
    fn user_token_path_does_not_consume_global() {
        let lim = DiscoverRateLimiters::new(2, 10, 25);
        // Exhaust user-token quota partially without touching global.
        for _ in 0..5 {
            lim.check_user_token(42).unwrap();
        }
        assert_eq!(lim.global_count(), 0);

        // Shared path still has full global budget.
        lim.check_shared(1).unwrap();
        lim.check_shared(1).unwrap();
        assert_eq!(lim.global_count(), 2);
        let err = lim.check_shared(2).expect_err("global exhausted by shared only");
        assert_eq!(err.scope, RateLimitScope::Global);

        // User-token path still works and still does not care about global.
        lim.check_user_token(42).unwrap();
        assert_eq!(lim.global_count(), 2);
    }

    #[test]
    fn user_token_exhaustion_is_user_scope() {
        let lim = DiscoverRateLimiters::new(100, 10, 3);
        lim.check_user_token(7).unwrap();
        lim.check_user_token(7).unwrap();
        lim.check_user_token(7).unwrap();
        let err = lim.check_user_token(7).expect_err("4th user-token");
        assert_eq!(err.scope, RateLimitScope::User);
        // Other user unaffected.
        lim.check_user_token(8).unwrap();
    }

    #[test]
    fn from_settings_reads_limits() {
        let settings = Settings::from_map(|k| match k {
            "DATABASE_URL" => Some("postgres://x".into()),
            "JWT_SECRET" => Some("s".into()),
            "DISCOVER_RATE_LIMIT_PER_MIN" => Some("3".into()),
            "DISCOVER_RATE_LIMIT_PER_USER_PER_MIN" => Some("50".into()),
            "DISCOVER_RATE_LIMIT_PER_USER_WITH_TOKEN_PER_MIN" => Some("1".into()),
            _ => None,
        })
        .unwrap();
        let lim = DiscoverRateLimiters::from_settings(&settings);
        lim.check_shared(1).unwrap();
        lim.check_shared(1).unwrap();
        lim.check_shared(1).unwrap();
        assert_eq!(
            lim.check_shared(1).unwrap_err().scope,
            RateLimitScope::Global
        );
        lim.check_user_token(9).unwrap();
        assert_eq!(
            lim.check_user_token(9).unwrap_err().scope,
            RateLimitScope::User
        );
    }
}
