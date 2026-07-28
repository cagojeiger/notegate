use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard, Weak};

use moka::future::Cache;
use moka::policy::EvictionPolicy;
use notegate_core::SearchBodyCacheConfig;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

use super::SearchBodyCacheStats;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BodyCacheKey {
    space_id: Uuid,
    node_id: Uuid,
    content_sha256: String,
}

impl BodyCacheKey {
    fn new(space_id: Uuid, node_id: Uuid, content_sha256: &str) -> Self {
        Self {
            space_id,
            node_id,
            content_sha256: content_sha256.to_owned(),
        }
    }
}

#[derive(Debug, Default)]
struct BodyLoadFlights {
    locks: StdMutex<HashMap<BodyCacheKey, Weak<AsyncMutex<()>>>>,
}

impl BodyLoadFlights {
    fn lock_map(&self) -> StdMutexGuard<'_, HashMap<BodyCacheKey, Weak<AsyncMutex<()>>>> {
        match self.locks.lock() {
            Ok(locks) => locks,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_for(&self, key: &BodyCacheKey) -> Arc<AsyncMutex<()>> {
        let mut locks = self.lock_map();
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            return lock;
        }

        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(key.clone(), Arc::downgrade(&lock));
        lock
    }

    fn remove_unused(&self, keys_and_locks: &[(BodyCacheKey, Arc<AsyncMutex<()>>)]) {
        let mut locks = self.lock_map();
        for (key, lock) in keys_and_locks {
            if Arc::strong_count(lock) != 1 {
                continue;
            }
            let points_to_same_lock = locks
                .get(key)
                .and_then(Weak::upgrade)
                .is_some_and(|registered| Arc::ptr_eq(&registered, lock));
            if points_to_same_lock {
                locks.remove(key);
            }
        }
    }
}

pub(super) struct BodyLoadGuard {
    flights: Arc<BodyLoadFlights>,
    keys_and_locks: Vec<(BodyCacheKey, Arc<AsyncMutex<()>>)>,
    guards: Vec<OwnedMutexGuard<()>>,
}

impl BodyLoadGuard {
    fn empty(flights: Arc<BodyLoadFlights>) -> Self {
        Self {
            flights,
            keys_and_locks: Vec::new(),
            guards: Vec::new(),
        }
    }
}

impl Drop for BodyLoadGuard {
    fn drop(&mut self) {
        self.guards.clear();
        self.flights.remove_unused(&self.keys_and_locks);
    }
}

#[derive(Debug, Clone)]
pub(super) struct SearchBodyCache {
    entries: Option<Cache<BodyCacheKey, Arc<str>>>,
    flights: Arc<BodyLoadFlights>,
}

impl SearchBodyCache {
    pub(super) fn new(config: SearchBodyCacheConfig) -> Self {
        let entries = (config.max_capacity_bytes > 0).then(|| {
            Cache::builder()
                .name("grep-decrypted-bodies")
                .max_capacity(config.max_capacity_bytes)
                .weigher(|_key: &BodyCacheKey, body: &Arc<str>| {
                    u32::try_from(body.len()).unwrap_or(u32::MAX).max(1)
                })
                .eviction_policy(EvictionPolicy::tiny_lfu())
                .time_to_live(config.ttl)
                .time_to_idle(config.tti)
                .build()
        });
        Self {
            entries,
            flights: Arc::new(BodyLoadFlights::default()),
        }
    }

    pub(super) fn stats(&self) -> SearchBodyCacheStats {
        let Some(entries) = &self.entries else {
            return SearchBodyCacheStats::default();
        };

        SearchBodyCacheStats {
            entries: entries.entry_count(),
            size_bytes: entries.weighted_size(),
            capacity_bytes: entries.policy().max_capacity().unwrap_or_default(),
        }
    }

    pub(super) async fn get(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        content_sha256: &str,
    ) -> Option<Arc<str>> {
        let entries = self.entries.as_ref()?;
        entries
            .get(&BodyCacheKey::new(space_id, node_id, content_sha256))
            .await
    }

    pub(super) async fn insert(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        content_sha256: &str,
        body: Arc<str>,
    ) {
        if let Some(entries) = &self.entries {
            entries
                .insert(BodyCacheKey::new(space_id, node_id, content_sha256), body)
                .await;
        }
    }

    pub(super) async fn lock_misses(
        &self,
        space_id: Uuid,
        candidates: &[(Uuid, String, i64)],
    ) -> BodyLoadGuard {
        if self.entries.is_none() || candidates.is_empty() {
            return BodyLoadGuard::empty(Arc::clone(&self.flights));
        }

        let mut keys = candidates
            .iter()
            .map(|(node_id, content_sha256, _byte_len)| {
                BodyCacheKey::new(space_id, *node_id, content_sha256)
            })
            .collect::<Vec<_>>();
        keys.sort_unstable();
        keys.dedup();

        let mut load_guard = BodyLoadGuard {
            flights: Arc::clone(&self.flights),
            keys_and_locks: Vec::with_capacity(keys.len()),
            guards: Vec::with_capacity(keys.len()),
        };
        for key in keys {
            let lock = self.flights.lock_for(&key);
            load_guard.keys_and_locks.push((key, Arc::clone(&lock)));
            load_guard.guards.push(lock.lock_owned().await);
        }

        load_guard
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::sync::Barrier;

    use super::*;

    #[tokio::test]
    async fn cache_uses_plaintext_bytes_and_configured_expiration_policy() {
        let config = SearchBodyCacheConfig {
            max_capacity_bytes: 8,
            ttl: Duration::from_secs(30 * 60),
            tti: Duration::from_secs(5 * 60),
        };
        let cache = SearchBodyCache::new(config);
        let entries = cache.entries.as_ref().expect("cache enabled");

        assert_eq!(entries.policy().max_capacity(), Some(8));
        assert_eq!(entries.policy().time_to_live(), Some(config.ttl));
        assert_eq!(entries.policy().time_to_idle(), Some(config.tti));

        cache
            .insert(Uuid::nil(), Uuid::nil(), "sha", Arc::<str>::from("12345"))
            .await;
        entries.run_pending_tasks().await;
        assert_eq!(entries.weighted_size(), 5);
        assert_eq!(
            cache.stats(),
            SearchBodyCacheStats {
                entries: 1,
                size_bytes: 5,
                capacity_bytes: 8,
            }
        );
    }

    #[tokio::test]
    async fn zero_capacity_disables_storage() {
        let cache = SearchBodyCache::new(SearchBodyCacheConfig {
            max_capacity_bytes: 0,
            ..SearchBodyCacheConfig::default()
        });

        cache
            .insert(Uuid::nil(), Uuid::nil(), "sha", Arc::<str>::from("body"))
            .await;
        assert!(cache.get(Uuid::nil(), Uuid::nil(), "sha").await.is_none());
        assert_eq!(cache.stats(), SearchBodyCacheStats::default());
    }

    #[tokio::test]
    async fn concurrent_misses_for_the_same_key_have_one_loader() {
        let cache = SearchBodyCache::new(SearchBodyCacheConfig::default());
        let space_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let candidates = vec![(node_id, "sha".to_owned(), 4)];
        let barrier = Arc::new(Barrier::new(2));
        let loads = Arc::new(AtomicUsize::new(0));

        let mut tasks = Vec::new();
        for _ in 0..2 {
            let cache = cache.clone();
            let candidates = candidates.clone();
            let barrier = Arc::clone(&barrier);
            let loads = Arc::clone(&loads);
            tasks.push(tokio::spawn(async move {
                assert!(cache.get(space_id, node_id, "sha").await.is_none());
                barrier.wait().await;

                let _guard = cache.lock_misses(space_id, &candidates).await;
                if let Some(body) = cache.get(space_id, node_id, "sha").await {
                    return body;
                }

                loads.fetch_add(1, Ordering::SeqCst);
                let body = Arc::<str>::from("body");
                cache
                    .insert(space_id, node_id, "sha", Arc::clone(&body))
                    .await;
                body
            }));
        }

        for task in tasks {
            assert_eq!(&*task.await.expect("cache task"), "body");
        }
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }
}
