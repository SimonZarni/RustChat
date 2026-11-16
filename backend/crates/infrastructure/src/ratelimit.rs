//! Rate-limit implementations.
//!
//! Production uses expiring, PostgreSQL-backed fixed-window counters shared by
//! servers using the same database. An in-memory implementation remains for
//! tests; its state is per process and bounded by a maximum key count.

use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};

use async_trait::async_trait;
use rustchat_application::{
    error::AppError,
    ports::{RateLimitDecision, RateLimitRule, RateLimiter},
};
use time::OffsetDateTime;

/// PostgreSQL-backed counters shared by every server using the same database.
/// Keys are stored as SHA-256 digests so usernames and client addresses are
/// not retained in the rate-limit table.
pub struct PgRateLimiter {
    pool: sqlx::PgPool,
}

impl PgRateLimiter {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    fn digest(key: &str) -> [u8; 32] {
        Sha256::digest(key.as_bytes()).into()
    }
}

#[async_trait]
impl RateLimiter for PgRateLimiter {
    async fn check(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<RateLimitDecision, AppError> {
        let entry = sqlx::query_as::<_, (i64, OffsetDateTime)>(
            "SELECT count, expires_at FROM rate_limit_entries WHERE key_digest = $1",
        )
        .bind(Self::digest(key).as_slice())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::internal)?;

        Ok(match entry {
            Some((count, expires_at)) if expires_at > now && count >= i64::from(rule.max) => {
                RateLimitDecision::Limited {
                    retry_after: expires_at - now,
                }
            }
            _ => RateLimitDecision::Allowed,
        })
    }

    async fn record(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let expires_at = now + rule.window;
        sqlx::query(
            "WITH cleanup AS (DELETE FROM rate_limit_entries WHERE expires_at <= $2) \
             INSERT INTO rate_limit_entries (key_digest, count, expires_at) \
             VALUES ($1, 1, $3) \
             ON CONFLICT (key_digest) DO UPDATE SET \
                 count = CASE WHEN rate_limit_entries.expires_at <= $2 \
                              THEN 1 ELSE rate_limit_entries.count + 1 END, \
                 expires_at = CASE WHEN rate_limit_entries.expires_at <= $2 \
                                   THEN $3 ELSE rate_limit_entries.expires_at END",
        )
        .bind(Self::digest(key).as_slice())
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(AppError::internal)?;
        Ok(())
    }

    async fn reset(&self, key: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM rate_limit_entries WHERE key_digest = $1")
            .bind(Self::digest(key).as_slice())
            .execute(&self.pool)
            .await
            .map_err(AppError::internal)?;
        Ok(())
    }
}

/// Default cap on tracked keys (a few MiB at most).
pub const DEFAULT_MAX_KEYS: usize = 100_000;

struct Entry {
    count: u32,
    expires_at: OffsetDateTime,
}

pub struct InMemoryRateLimiter {
    entries: Mutex<HashMap<String, Entry>>,
    max_keys: usize,
}

impl InMemoryRateLimiter {
    pub fn new() -> Self {
        Self::with_max_keys(DEFAULT_MAX_KEYS)
    }

    pub fn with_max_keys(max_keys: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            max_keys: max_keys.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        // The counters stay consistent even if a holder panicked.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Number of tracked keys (for tests and diagnostics).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemoryRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Make room for one more key: drop expired entries, then, if still full,
/// the entry closest to expiring. Evicting under extreme key pressure weakens
/// limits only for the evicted key, which is preferable to unbounded memory
/// or to refusing every new client.
fn make_room(entries: &mut HashMap<String, Entry>, max_keys: usize, now: OffsetDateTime) {
    if entries.len() < max_keys {
        return;
    }
    entries.retain(|_, entry| entry.expires_at > now);
    if entries.len() < max_keys {
        return;
    }
    let oldest = entries
        .iter()
        .min_by_key(|(_, entry)| entry.expires_at)
        .map(|(key, _)| key.clone());
    if let Some(key) = oldest {
        entries.remove(&key);
    }
}

#[async_trait]
impl RateLimiter for InMemoryRateLimiter {
    async fn check(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<RateLimitDecision, AppError> {
        let entries = self.lock();
        Ok(match entries.get(key) {
            Some(entry) if entry.expires_at > now && entry.count >= rule.max => {
                RateLimitDecision::Limited {
                    retry_after: entry.expires_at - now,
                }
            }
            _ => RateLimitDecision::Allowed,
        })
    }

    async fn record(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut entries = self.lock();
        if let Some(entry) = entries.get_mut(key) {
            if entry.expires_at > now {
                entry.count = entry.count.saturating_add(1);
                return Ok(());
            }
            // The previous window ended: start a fresh one.
            *entry = Entry {
                count: 1,
                expires_at: now + rule.window,
            };
            return Ok(());
        }
        make_room(&mut entries, self.max_keys, now);
        entries.insert(
            key.to_owned(),
            Entry {
                count: 1,
                expires_at: now + rule.window,
            },
        );
        Ok(())
    }

    async fn reset(&self, key: &str) -> Result<(), AppError> {
        self.lock().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    const T0: OffsetDateTime = OffsetDateTime::UNIX_EPOCH;

    fn rule(max: u32, secs: i64) -> RateLimitRule {
        RateLimitRule {
            max,
            window: Duration::seconds(secs),
        }
    }

    async fn allowed(l: &InMemoryRateLimiter, key: &str, r: RateLimitRule, at: i64) -> bool {
        matches!(
            l.check(key, r, T0 + Duration::seconds(at)).await.unwrap(),
            RateLimitDecision::Allowed
        )
    }

    async fn hit(l: &InMemoryRateLimiter, key: &str, r: RateLimitRule, at: i64) {
        l.record(key, r, T0 + Duration::seconds(at)).await.unwrap();
    }

    #[tokio::test]
    async fn allows_up_to_max_then_limits() {
        let l = InMemoryRateLimiter::new();
        let r = rule(3, 60);
        for _ in 0..3 {
            assert!(allowed(&l, "k", r, 0).await);
            hit(&l, "k", r, 0).await;
        }
        assert!(!allowed(&l, "k", r, 1).await);
    }

    #[tokio::test]
    async fn check_does_not_consume_budget() {
        let l = InMemoryRateLimiter::new();
        let r = rule(1, 60);
        for _ in 0..10 {
            assert!(allowed(&l, "k", r, 0).await);
        }
        assert!(l.is_empty());
    }

    #[tokio::test]
    async fn reports_the_time_until_the_window_ends() {
        let l = InMemoryRateLimiter::new();
        let r = rule(1, 60);
        hit(&l, "k", r, 10).await;
        let decision = l.check("k", r, T0 + Duration::seconds(25)).await.unwrap();
        assert_eq!(
            decision,
            RateLimitDecision::Limited {
                retry_after: Duration::seconds(45)
            }
        );
    }

    #[tokio::test]
    async fn the_window_expires_and_counting_restarts() {
        let l = InMemoryRateLimiter::new();
        let r = rule(2, 60);
        hit(&l, "k", r, 0).await;
        hit(&l, "k", r, 5).await;
        assert!(!allowed(&l, "k", r, 59).await);
        assert!(allowed(&l, "k", r, 60).await, "window end is exclusive");

        hit(&l, "k", r, 61).await; // starts a new window with count 1
        assert!(allowed(&l, "k", r, 62).await);
        hit(&l, "k", r, 62).await;
        assert!(!allowed(&l, "k", r, 63).await);
    }

    #[tokio::test]
    async fn keys_are_independent() {
        let l = InMemoryRateLimiter::new();
        let r = rule(1, 60);
        hit(&l, "a", r, 0).await;
        assert!(!allowed(&l, "a", r, 1).await);
        assert!(allowed(&l, "b", r, 1).await);
    }

    #[tokio::test]
    async fn reset_clears_a_key_only() {
        let l = InMemoryRateLimiter::new();
        let r = rule(1, 60);
        hit(&l, "a", r, 0).await;
        hit(&l, "b", r, 0).await;
        l.reset("a").await.unwrap();
        assert!(allowed(&l, "a", r, 1).await);
        assert!(!allowed(&l, "b", r, 1).await);
        l.reset("never-seen").await.unwrap();
    }

    #[tokio::test]
    async fn memory_is_bounded_by_max_keys() {
        let l = InMemoryRateLimiter::with_max_keys(100);
        let r = rule(5, 3600);
        for i in 0..10_000 {
            hit(&l, &format!("attacker-key-{i}"), r, i64::from(i % 50)).await;
            assert!(l.len() <= 100, "grew to {} at {i}", l.len());
        }
        assert_eq!(l.len(), 100);
    }

    #[tokio::test]
    async fn expired_entries_are_dropped_before_live_ones_are_evicted() {
        let l = InMemoryRateLimiter::with_max_keys(3);
        let short = rule(1, 10);
        let long = rule(1, 1000);
        hit(&l, "short1", short, 0).await;
        hit(&l, "short2", short, 0).await;
        hit(&l, "long", long, 0).await;
        // Full; the two short windows have ended by t=20, so they go first.
        hit(&l, "new", long, 20).await;
        assert!(!allowed(&l, "long", long, 21).await, "live key was evicted");
        assert!(!allowed(&l, "new", long, 21).await);
        assert_eq!(l.len(), 2);
    }

    #[tokio::test]
    async fn counter_saturates_instead_of_overflowing() {
        let l = InMemoryRateLimiter::new();
        let r = rule(u32::MAX, 60);
        {
            let mut entries = l.lock();
            entries.insert(
                "k".into(),
                Entry {
                    count: u32::MAX,
                    expires_at: T0 + Duration::seconds(60),
                },
            );
        }
        hit(&l, "k", r, 1).await;
        assert!(!allowed(&l, "k", r, 2).await);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn postgres_limits_are_shared_between_instances_and_resettable(pool: sqlx::PgPool) {
        let first = PgRateLimiter::new(pool.clone());
        let second = PgRateLimiter::new(pool);
        let r = rule(1, 60);
        let now = T0 + Duration::seconds(100);

        first.record("login:user:alice", r, now).await.unwrap();
        assert_eq!(
            second.check("login:user:alice", r, now).await.unwrap(),
            RateLimitDecision::Limited {
                retry_after: Duration::seconds(60)
            }
        );

        second.reset("login:user:alice").await.unwrap();
        assert_eq!(
            first.check("login:user:alice", r, now).await.unwrap(),
            RateLimitDecision::Allowed
        );
    }
}
