//! Shared lifecycle for periodic background workers.

use std::future::Future;
use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;

/// Run work immediately and then at `period` until shutdown is requested.
///
/// Cancellation wins over a ready tick, so no new run starts after shutdown.
/// Work that already started is allowed to finish before this function returns.
pub async fn run<F, Fut>(period: Duration, shutdown: CancellationToken, mut work: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut ticker = interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return,
            _ = ticker.tick() => work().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::Notify;

    use super::*;

    #[tokio::test]
    async fn cancelled_worker_does_not_start_work() {
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let runs = Arc::new(AtomicUsize::new(0));
        let observed_runs = runs.clone();

        run(Duration::from_secs(60), shutdown, move || {
            observed_runs.fetch_add(1, Ordering::SeqCst);
            async {}
        })
        .await;

        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancellation_drains_current_work_without_starting_another_run()
    -> Result<(), tokio::task::JoinError> {
        let shutdown = CancellationToken::new();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let runs = Arc::new(AtomicUsize::new(0));

        let worker = tokio::spawn({
            let shutdown = shutdown.clone();
            let started = started.clone();
            let release = release.clone();
            let runs = runs.clone();
            async move {
                run(Duration::from_millis(1), shutdown, move || {
                    let started = started.clone();
                    let release = release.clone();
                    runs.fetch_add(1, Ordering::SeqCst);
                    async move {
                        started.notify_one();
                        release.notified().await;
                    }
                })
                .await;
            }
        });

        started.notified().await;
        shutdown.cancel();
        tokio::task::yield_now().await;
        assert!(!worker.is_finished());

        release.notify_one();
        worker.await?;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
