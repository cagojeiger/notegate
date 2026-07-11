//! Operator command for rebuilding every live Space usage counter.
//!
//! Reads remain available during the run. File-tree mutations receive the
//! standard retryable maintenance error until the transaction commits.

use notegate_core::Config;
use notegate_db::{FullUsageReconcileRun, SpaceUsageRepo, connect, run_migrations};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let pool = connect(&config).await?;
    run_migrations(&pool).await?;

    match SpaceUsageRepo::new(pool).run_full_recalculation().await? {
        FullUsageReconcileRun::Recalculated {
            spaces_recalculated,
        } => println!("recalculated {spaces_recalculated} spaces"),
        FullUsageReconcileRun::WorkerLockHeld => {
            return Err("usage reconciliation is already running; retry later".into());
        }
        FullUsageReconcileRun::MutationsActive => {
            return Err("file-tree mutations are active; retry later".into());
        }
    }

    Ok(())
}
