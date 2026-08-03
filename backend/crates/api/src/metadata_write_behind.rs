//! Process-local coalescing for best-effort metadata writes.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use notegate_core::Result;
use notegate_db::{MediaTypeObservation, MetadataWriteRepo, PgPool};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_PENDING_PER_KIND: usize = 10_000;
const FINAL_FLUSH_ATTEMPTS: usize = 3;
const FINAL_FLUSH_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
struct PendingMediaType {
    media_type: String,
    observed_at: DateTime<Utc>,
}

#[derive(Debug)]
/// Reusable bounded, key-coalescing queue storage. A metadata kind supplies
/// only its key/value and merge rule; flush scheduling and failure recovery are
/// shared by the worker below.
struct CoalescingMap<K, V> {
    values: HashMap<K, V>,
    dropped: u64,
}

impl<K, V> Default for CoalescingMap<K, V> {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            dropped: 0,
        }
    }
}

impl<K: Eq + Hash, V> CoalescingMap<K, V> {
    fn record(&mut self, key: K, value: V, max_pending: usize, merge: impl FnOnce(&mut V, V)) {
        if let Some(current) = self.values.get_mut(&key) {
            merge(current, value);
            return;
        }
        if self.values.len() >= max_pending {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.values.insert(key, value);
    }

    fn take_values(&mut self) -> HashMap<K, V> {
        std::mem::take(&mut self.values)
    }

    fn restore_failed(
        &mut self,
        failed: HashMap<K, V>,
        max_pending: usize,
        merge: impl Fn(&mut V, V) + Copy,
    ) {
        let arrived_during_flush = std::mem::replace(&mut self.values, failed);
        for (key, value) in arrived_during_flush {
            self.record(key, value, max_pending, merge);
        }
    }

    fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }
}

#[derive(Debug)]
struct CoalescingBuffer<K, V> {
    pending: Mutex<CoalescingMap<K, V>>,
    max_pending: usize,
}

impl<K: Eq + Hash, V> CoalescingBuffer<K, V> {
    fn new(max_pending: usize) -> Self {
        Self {
            pending: Mutex::new(CoalescingMap::default()),
            max_pending,
        }
    }

    fn record(&self, key: K, value: V, merge: impl FnOnce(&mut V, V)) {
        self.lock().record(key, value, self.max_pending, merge);
    }

    fn take(&self) -> HashMap<K, V> {
        self.lock().take_values()
    }

    fn restore_failed(&self, failed: HashMap<K, V>, merge: impl Fn(&mut V, V) + Copy) {
        self.lock().restore_failed(failed, self.max_pending, merge);
    }

    fn is_empty(&self) -> bool {
        self.lock().values.is_empty()
    }

    fn len(&self) -> usize {
        self.lock().values.len()
    }

    fn take_dropped(&self) -> u64 {
        self.lock().take_dropped()
    }

    fn lock(&self) -> MutexGuard<'_, CoalescingMap<K, V>> {
        match self.pending.lock() {
            Ok(pending) => pending,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Debug)]
struct BufferInner {
    api_keys: CoalescingBuffer<Uuid, ()>,
    browser_sessions: CoalescingBuffer<Uuid, ()>,
    media_types: CoalescingBuffer<(Uuid, Uuid), PendingMediaType>,
}

#[derive(Debug, Clone)]
pub(crate) struct MetadataWriteBuffer {
    inner: Arc<BufferInner>,
}

impl Default for MetadataWriteBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PENDING_PER_KIND)
    }
}

impl MetadataWriteBuffer {
    fn new(max_pending_per_kind: usize) -> Self {
        Self {
            inner: Arc::new(BufferInner {
                api_keys: CoalescingBuffer::new(max_pending_per_kind),
                browser_sessions: CoalescingBuffer::new(max_pending_per_kind),
                media_types: CoalescingBuffer::new(max_pending_per_kind),
            }),
        }
    }

    pub(crate) fn record_api_key(&self, key_id: Uuid) {
        self.inner.api_keys.record(key_id, (), |_, ()| {});
    }

    pub(crate) fn record_browser_session(&self, session_id: Uuid) {
        self.inner
            .browser_sessions
            .record(session_id, (), |_, ()| {});
    }

    pub(crate) fn record_media_type(
        &self,
        space_id: Uuid,
        node_id: Uuid,
        media_type: String,
        observed_at: DateTime<Utc>,
    ) {
        self.inner.media_types.record(
            (space_id, node_id),
            PendingMediaType {
                media_type,
                observed_at,
            },
            merge_earliest_media,
        );
    }

    fn take(&self) -> MetadataBatch {
        MetadataBatch {
            api_keys: self.inner.api_keys.take(),
            browser_sessions: self.inner.browser_sessions.take(),
            media_types: self.inner.media_types.take(),
        }
    }

    fn requeue(&self, batch: MetadataBatch) {
        self.inner
            .api_keys
            .restore_failed(batch.api_keys, |_, ()| {});
        self.inner
            .browser_sessions
            .restore_failed(batch.browser_sessions, |_, ()| {});
        self.inner
            .media_types
            .restore_failed(batch.media_types, merge_earliest_media);
    }

    fn is_empty(&self) -> bool {
        self.inner.api_keys.is_empty()
            && self.inner.browser_sessions.is_empty()
            && self.inner.media_types.is_empty()
    }

    fn take_drop_counts(&self) -> (u64, u64, u64) {
        (
            self.inner.api_keys.take_dropped(),
            self.inner.browser_sessions.take_dropped(),
            self.inner.media_types.take_dropped(),
        )
    }

    fn pending_counts(&self) -> (usize, usize, usize) {
        (
            self.inner.api_keys.len(),
            self.inner.browser_sessions.len(),
            self.inner.media_types.len(),
        )
    }
}

fn merge_earliest_media(current: &mut PendingMediaType, observation: PendingMediaType) {
    if observation.observed_at < current.observed_at {
        *current = observation;
    }
}

#[derive(Debug, Default)]
struct MetadataBatch {
    api_keys: HashMap<Uuid, ()>,
    browser_sessions: HashMap<Uuid, ()>,
    media_types: HashMap<(Uuid, Uuid), PendingMediaType>,
}

impl MetadataBatch {
    fn is_empty(&self) -> bool {
        self.api_keys.is_empty() && self.browser_sessions.is_empty() && self.media_types.is_empty()
    }

    fn item_count(&self) -> usize {
        self.api_keys.len() + self.browser_sessions.len() + self.media_types.len()
    }

    fn counts(&self) -> (usize, usize, usize) {
        (
            self.api_keys.len(),
            self.browser_sessions.len(),
            self.media_types.len(),
        )
    }
}

type FlushFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

trait MetadataSink: Send + Sync + 'static {
    fn flush<'a>(&'a self, batch: &'a MetadataBatch) -> FlushFuture<'a>;
}

#[derive(Debug)]
struct DatabaseMetadataSink {
    repo: MetadataWriteRepo,
}

impl MetadataSink for DatabaseMetadataSink {
    fn flush<'a>(&'a self, batch: &'a MetadataBatch) -> FlushFuture<'a> {
        Box::pin(async move {
            let api_key_ids = batch.api_keys.keys().copied().collect::<Vec<_>>();
            let browser_session_ids = batch.browser_sessions.keys().copied().collect::<Vec<_>>();
            let media_types = batch
                .media_types
                .iter()
                .map(|(&(space_id, node_id), observation)| MediaTypeObservation {
                    space_id,
                    node_id,
                    media_type: observation.media_type.clone(),
                    observed_at: observation.observed_at,
                })
                .collect::<Vec<_>>();
            let (_, _, _) = tokio::try_join!(
                self.repo.update_api_key_last_used(&api_key_ids),
                self.repo
                    .update_browser_session_last_used(&browser_session_ids),
                self.repo.set_detected_media_types(&media_types),
            )?;
            Ok(())
        })
    }
}

pub(crate) fn spawn(
    buffer: MetadataWriteBuffer,
    pool: PgPool,
    shutdown: CancellationToken,
    metrics_enabled: bool,
) -> JoinHandle<()> {
    tokio::spawn(run(
        buffer,
        DatabaseMetadataSink {
            repo: MetadataWriteRepo::new(pool),
        },
        shutdown,
        FLUSH_INTERVAL,
        FLUSH_TIMEOUT,
        metrics_enabled,
    ))
}

#[cfg(test)]
pub(crate) async fn flush_for_test(buffer: &MetadataWriteBuffer, pool: PgPool) -> bool {
    flush_once(
        buffer,
        &DatabaseMetadataSink {
            repo: MetadataWriteRepo::new(pool),
        },
        FLUSH_TIMEOUT,
        false,
    )
    .await
}

async fn run<S: MetadataSink>(
    buffer: MetadataWriteBuffer,
    sink: S,
    shutdown: CancellationToken,
    flush_interval: Duration,
    flush_timeout: Duration,
    metrics_enabled: bool,
) {
    tracing::info!(event = "metadata_write_behind.started");
    let mut ticker = interval_at(Instant::now() + flush_interval, flush_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                let _ = flush_once(&buffer, &sink, flush_timeout, metrics_enabled).await;
            }
        }
    }

    for attempt in 1..=FINAL_FLUSH_ATTEMPTS {
        if buffer.is_empty() || flush_once(&buffer, &sink, flush_timeout, metrics_enabled).await {
            break;
        }
        if attempt < FINAL_FLUSH_ATTEMPTS {
            sleep(FINAL_FLUSH_RETRY_DELAY).await;
        }
    }
    report_pending(&buffer, metrics_enabled);
    report_drops(&buffer, metrics_enabled);
    tracing::info!(event = "metadata_write_behind.stopped");
}

async fn flush_once<S: MetadataSink>(
    buffer: &MetadataWriteBuffer,
    sink: &S,
    flush_timeout: Duration,
    metrics_enabled: bool,
) -> bool {
    let batch = buffer.take();
    if batch.is_empty() {
        report_drops(buffer, metrics_enabled);
        return true;
    }
    let item_count = batch.item_count();
    let counts = batch.counts();
    let started_at = metrics_enabled.then(Instant::now);
    match timeout(flush_timeout, sink.flush(&batch)).await {
        Ok(Ok(())) => {
            tracing::debug!(event = "metadata_write_behind.flushed", item_count);
            report_flush_metrics(metrics_enabled, "success", started_at, counts);
            report_drops(buffer, metrics_enabled);
            true
        }
        Ok(Err(error)) => {
            buffer.requeue(batch);
            tracing::warn!(event = "metadata_write_behind.flush_failed", item_count, %error);
            report_flush_metrics(metrics_enabled, "error", started_at, (0, 0, 0));
            false
        }
        Err(_elapsed) => {
            buffer.requeue(batch);
            tracing::warn!(
                event = "metadata_write_behind.flush_timed_out",
                item_count,
                timeout_ms = flush_timeout.as_millis(),
            );
            report_flush_metrics(metrics_enabled, "timeout", started_at, (0, 0, 0));
            false
        }
    }
}

fn report_flush_metrics(
    metrics_enabled: bool,
    outcome: &'static str,
    started_at: Option<Instant>,
    flushed: (usize, usize, usize),
) {
    if let Some(started_at) = started_at {
        crate::observability::record_metadata_flush(metrics_enabled, outcome, started_at.elapsed());
    }
    let (api_keys, browser_sessions, media_types) = flushed;
    crate::observability::record_metadata_items(
        metrics_enabled,
        "api_key",
        "flushed",
        u64::try_from(api_keys).unwrap_or(u64::MAX),
    );
    crate::observability::record_metadata_items(
        metrics_enabled,
        "browser_session",
        "flushed",
        u64::try_from(browser_sessions).unwrap_or(u64::MAX),
    );
    crate::observability::record_metadata_items(
        metrics_enabled,
        "media_type",
        "flushed",
        u64::try_from(media_types).unwrap_or(u64::MAX),
    );
}

fn report_pending(buffer: &MetadataWriteBuffer, metrics_enabled: bool) {
    let (api_keys, browser_sessions, media_types) = buffer.pending_counts();
    if api_keys + browser_sessions + media_types > 0 {
        tracing::error!(
            event = "metadata_write_behind.shutdown_incomplete",
            api_keys,
            browser_sessions,
            media_types,
        );
        crate::observability::record_metadata_items(
            metrics_enabled,
            "api_key",
            "stranded",
            u64::try_from(api_keys).unwrap_or(u64::MAX),
        );
        crate::observability::record_metadata_items(
            metrics_enabled,
            "browser_session",
            "stranded",
            u64::try_from(browser_sessions).unwrap_or(u64::MAX),
        );
        crate::observability::record_metadata_items(
            metrics_enabled,
            "media_type",
            "stranded",
            u64::try_from(media_types).unwrap_or(u64::MAX),
        );
    }
}

fn report_drops(buffer: &MetadataWriteBuffer, metrics_enabled: bool) {
    let (api_keys, browser_sessions, media_types) = buffer.take_drop_counts();
    if api_keys + browser_sessions + media_types > 0 {
        tracing::warn!(
            event = "metadata_write_behind.dropped",
            api_keys,
            browser_sessions,
            media_types,
        );
        crate::observability::record_metadata_items(
            metrics_enabled,
            "api_key",
            "dropped",
            api_keys,
        );
        crate::observability::record_metadata_items(
            metrics_enabled,
            "browser_session",
            "dropped",
            browser_sessions,
        );
        crate::observability::record_metadata_items(
            metrics_enabled,
            "media_type",
            "dropped",
            media_types,
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]

    use std::sync::atomic::{AtomicUsize, Ordering};

    use notegate_core::Error;

    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        attempts: AtomicUsize,
        fail_attempts: usize,
        batches: Mutex<Vec<usize>>,
    }

    impl MetadataSink for RecordingSink {
        fn flush<'a>(&'a self, batch: &'a MetadataBatch) -> FlushFuture<'a> {
            Box::pin(async move {
                let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt <= self.fail_attempts {
                    return Err(Error::internal("test flush failure"));
                }
                match self.batches.lock() {
                    Ok(mut batches) => batches.push(batch.item_count()),
                    Err(poisoned) => poisoned.into_inner().push(batch.item_count()),
                }
                Ok(())
            })
        }
    }

    #[test]
    fn activity_write_behind_coalesces_repeated_usage_and_earliest_media_detection() {
        let buffer = MetadataWriteBuffer::new(10);
        let key_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let space_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let earlier = Utc::now();
        let later = earlier + chrono::Duration::seconds(1);

        buffer.record_api_key(key_id);
        buffer.record_api_key(key_id);
        buffer.record_browser_session(session_id);
        buffer.record_browser_session(session_id);
        buffer.record_media_type(space_id, node_id, "image/png".to_owned(), later);
        buffer.record_media_type(space_id, node_id, "image/jpeg".to_owned(), earlier);

        let batch = buffer.take();
        assert_eq!(batch.api_keys.len(), 1);
        assert!(batch.api_keys.contains_key(&key_id));
        assert_eq!(batch.browser_sessions.len(), 1);
        assert!(batch.browser_sessions.contains_key(&session_id));
        assert_eq!(batch.media_types.len(), 1);
        assert_eq!(
            batch
                .media_types
                .get(&(space_id, node_id))
                .map(|observation| observation.media_type.as_str()),
            Some("image/jpeg")
        );
    }

    #[test]
    fn activity_write_behind_bounds_distinct_keys_but_keeps_existing_keys_fresh() {
        let buffer = MetadataWriteBuffer::new(1);
        let retained = Uuid::new_v4();
        let dropped = Uuid::new_v4();
        buffer.record_api_key(retained);
        buffer.record_api_key(dropped);
        buffer.record_api_key(retained);

        let batch = buffer.take();
        assert_eq!(batch.api_keys.len(), 1);
        assert!(batch.api_keys.contains_key(&retained));
        assert_eq!(buffer.inner.api_keys.lock().dropped, 1);
    }

    #[test]
    fn activity_write_behind_requeue_preserves_failed_batch_before_new_distinct_keys() {
        let buffer = MetadataWriteBuffer::new(1);
        let failed_id = Uuid::new_v4();
        let arrived_id = Uuid::new_v4();
        buffer.record_api_key(failed_id);
        let failed_batch = buffer.take();

        buffer.record_api_key(arrived_id);
        buffer.requeue(failed_batch);

        let retried = buffer.take();
        assert_eq!(retried.api_keys.len(), 1);
        assert!(retried.api_keys.contains_key(&failed_id));
        assert_eq!(buffer.inner.api_keys.lock().dropped, 1);
    }

    #[tokio::test]
    async fn activity_write_behind_failed_flush_is_requeued_and_retryable() {
        let buffer = MetadataWriteBuffer::new(10);
        buffer.record_api_key(Uuid::new_v4());
        let sink = RecordingSink {
            fail_attempts: 1,
            ..RecordingSink::default()
        };

        assert!(!flush_once(&buffer, &sink, Duration::from_secs(1), false).await);
        assert!(!buffer.is_empty());
        assert!(flush_once(&buffer, &sink, Duration::from_secs(1), false).await);
        assert!(buffer.is_empty());
        assert_eq!(sink.attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn activity_write_behind_shutdown_drains_pending_values() {
        let buffer = MetadataWriteBuffer::new(10);
        buffer.record_browser_session(Uuid::new_v4());
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let sink = RecordingSink::default();

        run(
            buffer.clone(),
            sink,
            shutdown,
            Duration::from_secs(60),
            Duration::from_secs(1),
            false,
        )
        .await;

        assert!(buffer.is_empty());
    }

    struct HangingSink;

    impl MetadataSink for HangingSink {
        fn flush<'a>(&'a self, _batch: &'a MetadataBatch) -> FlushFuture<'a> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn activity_write_behind_times_out_and_requeues_a_stuck_flush() {
        let buffer = MetadataWriteBuffer::new(10);
        buffer.record_api_key(Uuid::new_v4());

        assert!(
            !flush_once(&buffer, &HangingSink, Duration::from_millis(10), false).await,
            "a timed-out flush must report failure"
        );
        assert!(!buffer.is_empty(), "a timed-out batch must be retryable");
    }
}
