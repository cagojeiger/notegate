//! Operator command for rebuilding every live Space usage counter.
//!
//! Reads remain available during the run. File-tree mutations receive the
//! standard retryable maintenance error until the transaction commits.

use notegate_core::Config;
use notegate_db::{FullUsageReconcileRun, SpaceUsageRepo, connect, run_migrations};

const MAX_ATTEMPTS: usize = 30;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let pool = connect(&config).await?;
    run_migrations(&pool).await?;

    let repo = SpaceUsageRepo::new(pool);
    for attempt in 1..=MAX_ATTEMPTS {
        match repo.run_full_recalculation().await? {
            FullUsageReconcileRun::Recalculated {
                spaces_recalculated,
            } => {
                println!("recalculated {spaces_recalculated} spaces");
                return Ok(());
            }
            FullUsageReconcileRun::WorkerLockHeld | FullUsageReconcileRun::MutationsActive
                if attempt < MAX_ATTEMPTS =>
            {
                tokio::time::sleep(RETRY_DELAY).await;
            }
            FullUsageReconcileRun::WorkerLockHeld => {
                return Err("usage reconciliation remained busy; retry later".into());
            }
            FullUsageReconcileRun::MutationsActive => {
                return Err("file-tree mutations remained active; retry later".into());
            }
        }
    }

    Err("full usage recalculation did not complete".into())
}
