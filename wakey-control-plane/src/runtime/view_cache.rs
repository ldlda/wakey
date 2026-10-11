//! Lazy derived views keyed by their inputs, with one rebuild at a time.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::Mutex;

const MAX_REBUILDS: usize = 3;

#[derive(Debug)]
pub(crate) struct RebuildLimit;

impl std::fmt::Display for RebuildLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("view inputs kept changing during rebuilding; retry the request")
    }
}

impl std::error::Error for RebuildLimit {}

pub(crate) struct ViewCache<K, V> {
    entry: Mutex<Option<(K, Arc<V>)>>,
}

impl<K, V> Default for ViewCache<K, V> {
    fn default() -> Self {
        Self {
            entry: Mutex::new(None),
        }
    }
}

impl<K: Eq, V> ViewCache<K, V> {
    /// Reuse a matching view or rebuild it. `key` must reflect every input to
    /// `build`; rechecking it prevents an in-flight old build being published.
    /// Errors are not cached, and cancellation releases the rebuild lock.
    /// Three invalidated builds return `RebuildLimit` rather than starving readers.
    pub(crate) async fn get_or_try_init<KF, BF>(
        &self,
        key: impl Fn() -> KF,
        build: impl Fn() -> BF,
    ) -> anyhow::Result<Arc<V>>
    where
        KF: Future<Output = K>,
        BF: Future<Output = anyhow::Result<V>>,
    {
        // Keep publication and rebuilding under one lock. Waiting readers then
        // reuse the completed view rather than launching duplicate DB work.
        let mut entry = self.entry.lock().await;
        for _ in 0..MAX_REBUILDS {
            let before = key().await;
            if let Some((stored_key, value)) = &*entry
                && *stored_key == before
            {
                return Ok(Arc::clone(value));
            }
            let value = build().await?;
            if before == key().await {
                let value = Arc::new(value);
                *entry = Some((before, Arc::clone(&value)));
                return Ok(value);
            }
        }
        Err(RebuildLimit.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::Notify;

    #[tokio::test]
    async fn concurrent_readers_share_a_build_and_changed_keys_rebuild() {
        let cache = ViewCache::default();
        let revision = AtomicU64::new(0);
        let builds = AtomicU64::new(0);
        let key = || async { revision.load(Ordering::SeqCst) };
        let build = || async {
            builds.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok::<_, anyhow::Error>(42)
        };
        let (a, b) = tokio::join!(
            cache.get_or_try_init(key, build),
            cache.get_or_try_init(key, build)
        );
        assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        revision.fetch_add(1, Ordering::SeqCst);
        cache.get_or_try_init(key, build).await.unwrap();
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn invalidation_during_a_build_discards_the_old_result() {
        let cache = ViewCache::default();
        let revision = AtomicU64::new(0);
        let builds = AtomicU64::new(0);
        let started = Notify::new();
        let release = Notify::new();
        let reader = cache.get_or_try_init(
            || async { revision.load(Ordering::SeqCst) },
            || async {
                let captured = revision.load(Ordering::SeqCst);
                if builds.fetch_add(1, Ordering::SeqCst) == 0 {
                    started.notify_one();
                    release.notified().await;
                }
                Ok::<_, anyhow::Error>(captured)
            },
        );
        let writer = async {
            started.notified().await;
            revision.store(1, Ordering::SeqCst);
            release.notify_one();
        };
        let (result, ()) = tokio::join!(reader, writer);
        assert_eq!(*result.unwrap(), 1);
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_builds_are_retried() {
        let cache = ViewCache::default();
        assert!(
            cache
                .get_or_try_init(
                    || async { 0 },
                    || async { Err::<u8, _>(anyhow::anyhow!("db failed")) }
                )
                .await
                .is_err()
        );
        assert_eq!(
            *cache
                .get_or_try_init(|| async { 0 }, || async { Ok::<_, anyhow::Error>(7) })
                .await
                .unwrap(),
            7
        );
    }

    #[tokio::test]
    async fn cancelling_a_build_does_not_lock_out_readers() {
        let cache = Arc::new(ViewCache::default());
        let started = Arc::new(Notify::new());
        let task = tokio::spawn({
            let cache = Arc::clone(&cache);
            let started = Arc::clone(&started);
            async move {
                cache
                    .get_or_try_init(
                        || async { 0 },
                        || async {
                            started.notify_one();
                            std::future::pending::<anyhow::Result<u8>>().await
                        },
                    )
                    .await
            }
        });
        started.notified().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            *cache
                .get_or_try_init(|| async { 0 }, || async { Ok::<_, anyhow::Error>(7) })
                .await
                .unwrap(),
            7
        );
    }

    #[tokio::test]
    async fn sustained_changes_stop_at_the_limit_and_a_later_read_recovers() {
        let cache = ViewCache::default();
        let revision = AtomicU64::new(0);
        let key = || async { revision.load(Ordering::SeqCst) };
        cache
            .get_or_try_init(key, || async { Ok(0_u8) })
            .await
            .unwrap();
        revision.store(1, Ordering::SeqCst);
        let builds = AtomicU64::new(0);
        let error = cache
            .get_or_try_init(key, || async {
                builds.fetch_add(1, Ordering::SeqCst);
                revision.fetch_add(1, Ordering::SeqCst);
                Ok(1)
            })
            .await
            .unwrap_err();
        assert!(error.is::<RebuildLimit>());
        assert_eq!(builds.load(Ordering::SeqCst), MAX_REBUILDS as u64);
        let recovered = cache
            .get_or_try_init(key, || async { Ok(2) })
            .await
            .unwrap();
        assert_eq!(
            *recovered, 2,
            "must not fall back to the previous cached value"
        );
    }
}
