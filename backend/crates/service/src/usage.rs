//! User-facing quota and usage views.

use notegate_core::limits::Limits;
use notegate_core::tier::{UserTier, effective_file_tree_limits};
use notegate_db::{
    UsageReconciliationOutcome as DbUsageReconciliationOutcome, UsageRepo, UserUsageSnapshot,
};
use notegate_model::AccountKind;
use uuid::Uuid;

use crate::error::{ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct UsageService {
    store: UsageRepo,
    runtime_limits: Limits,
}

impl UsageService {
    pub fn new(store: UsageRepo, runtime_limits: Limits) -> Self {
        Self {
            store,
            runtime_limits,
        }
    }

    pub async fn current_user(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
    ) -> ServiceResult<CurrentUserUsage> {
        require_user_caller(caller_kind)?;
        let snapshot = self
            .store
            .current_user_usage(caller_account_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("user account not found".to_owned()))?;
        Ok(build_usage(snapshot, self.runtime_limits))
    }

    pub async fn request_space_reconciliation(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        space_id: Uuid,
    ) -> ServiceResult<UsageReconciliationOutcome> {
        require_user_caller(caller_kind)?;
        let outcome = self
            .store
            .request_space_reconciliation(caller_account_id, space_id)
            .await?;
        Ok(outcome.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageReconciliationOutcome {
    Queued,
    AlreadyQueued,
    Cooldown,
}

impl From<DbUsageReconciliationOutcome> for UsageReconciliationOutcome {
    fn from(outcome: DbUsageReconciliationOutcome) -> Self {
        match outcome {
            DbUsageReconciliationOutcome::Queued => Self::Queued,
            DbUsageReconciliationOutcome::AlreadyQueued => Self::AlreadyQueued,
            DbUsageReconciliationOutcome::Cooldown => Self::Cooldown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaUsage {
    pub used: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceUsage {
    pub id: Uuid,
    pub name: String,
    pub items: QuotaUsage,
    pub text_bytes: QuotaUsage,
    pub file_bytes: QuotaUsage,
    pub reconciliation_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUserUsage {
    pub tier: UserTier,
    pub spaces: Vec<SpaceUsage>,
}

fn build_usage(snapshot: UserUsageSnapshot, runtime_limits: Limits) -> CurrentUserUsage {
    let limits = effective_file_tree_limits(snapshot.tier, runtime_limits);
    let spaces = snapshot
        .spaces
        .into_iter()
        .map(|space| SpaceUsage {
            id: space.id,
            name: space.name,
            items: QuotaUsage {
                used: space.live_nodes.saturating_sub(1),
                limit: limits.space_max_nodes.saturating_sub(1),
            },
            text_bytes: QuotaUsage {
                used: space.live_text_bytes,
                limit: limits.space_max_text_bytes,
            },
            file_bytes: QuotaUsage {
                used: space.live_file_bytes,
                limit: limits.space_max_file_bytes,
            },
            reconciliation_pending: space.reconciliation_pending,
        })
        .collect();

    CurrentUserUsage {
        tier: snapshot.tier,
        spaces,
    }
}

fn require_user_caller(kind: AccountKind) -> ServiceResult<()> {
    match kind {
        AccountKind::User => Ok(()),
        AccountKind::Agent => Err(ServiceError::Forbidden(
            "only user accounts may view or reconcile usage".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use notegate_db::{SpaceUsageSnapshot, UserUsageSnapshot};

    use super::*;

    #[test]
    fn usage_applies_space_tier_and_runtime_limits() {
        let space_id = Uuid::new_v4();
        let usage = build_usage(
            UserUsageSnapshot {
                tier: UserTier::Tier0,
                spaces: vec![SpaceUsageSnapshot {
                    id: space_id,
                    name: "Personal".to_owned(),
                    live_nodes: 7,
                    live_text_bytes: 512,
                    live_file_bytes: 128,
                    reconciliation_pending: true,
                }],
            },
            Limits {
                space_max_nodes: 5,
                space_max_text_bytes: 256,
                space_max_file_bytes: 64,
                folder_max_children: 10,
            },
        );

        assert_eq!(usage.tier, UserTier::Tier0);
        assert_eq!(usage.spaces[0].id, space_id);
        assert_eq!(usage.spaces[0].items, QuotaUsage { used: 6, limit: 4 });
        assert_eq!(
            usage.spaces[0].text_bytes,
            QuotaUsage {
                used: 512,
                limit: 256,
            }
        );
        assert_eq!(
            usage.spaces[0].file_bytes,
            QuotaUsage {
                used: 128,
                limit: 64,
            }
        );
        assert!(usage.spaces[0].reconciliation_pending);
    }

    #[test]
    fn usage_rejects_agent_callers_before_repository_access() {
        assert!(matches!(
            require_user_caller(AccountKind::Agent),
            Err(ServiceError::Forbidden(_))
        ));
    }

    #[test]
    fn usage_excludes_the_space_root_from_items() {
        let usage = build_usage(
            UserUsageSnapshot {
                tier: UserTier::Tier0,
                spaces: vec![SpaceUsageSnapshot {
                    id: Uuid::new_v4(),
                    name: "Empty".to_owned(),
                    live_nodes: 1,
                    live_text_bytes: 0,
                    live_file_bytes: 0,
                    reconciliation_pending: false,
                }],
            },
            Limits {
                space_max_nodes: 1,
                space_max_text_bytes: 1,
                space_max_file_bytes: 1,
                folder_max_children: 1,
            },
        );

        assert_eq!(usage.spaces[0].items, QuotaUsage { used: 0, limit: 0 });
    }
}
