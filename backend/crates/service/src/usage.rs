//! User-facing quota and usage views.

use chrono::{DateTime, Utc};
use notegate_core::limits::{self, Limits};
use notegate_core::tier::{UserTier, effective_file_tree_limits};
use notegate_db::{UsageRepo, UserUsageSnapshot};
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
    ) -> ServiceResult<()> {
        require_user_caller(caller_kind)?;
        self.store
            .request_space_reconciliation(caller_account_id, space_id)
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaUsage {
    pub used: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountUsage {
    pub spaces: QuotaUsage,
    pub agents: QuotaUsage,
    pub api_keys: QuotaUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceUsage {
    pub id: Uuid,
    pub name: String,
    pub nodes: QuotaUsage,
    pub content_bytes: QuotaUsage,
    pub agent_connections: QuotaUsage,
    pub reconciliation: SpaceReconciliation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceReconciliation {
    pub pending: bool,
    pub reconciled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUserUsage {
    pub tier: UserTier,
    pub account: AccountUsage,
    pub spaces: Vec<SpaceUsage>,
}

fn build_usage(snapshot: UserUsageSnapshot, runtime_limits: Limits) -> CurrentUserUsage {
    let quota = snapshot.tier.quota();
    let file_limits = effective_file_tree_limits(snapshot.tier, runtime_limits);
    let spaces_used = snapshot.spaces.len();
    let spaces = snapshot
        .spaces
        .into_iter()
        .map(|space| SpaceUsage {
            id: space.id,
            name: space.name,
            nodes: QuotaUsage {
                used: space.live_nodes,
                limit: file_limits.space_max_nodes,
            },
            content_bytes: QuotaUsage {
                used: space.live_content_bytes,
                limit: file_limits.space_max_content_bytes,
            },
            agent_connections: QuotaUsage {
                used: space.live_agent_connections,
                limit: quota.connections_per_space,
            },
            reconciliation: SpaceReconciliation {
                pending: space.reconciliation_pending,
                reconciled_at: space.reconciled_at,
            },
        })
        .collect();

    CurrentUserUsage {
        tier: snapshot.tier,
        account: AccountUsage {
            spaces: QuotaUsage {
                used: spaces_used,
                limit: quota.spaces_per_user,
            },
            agents: QuotaUsage {
                used: snapshot.live_agents,
                limit: quota.agents_per_user,
            },
            api_keys: QuotaUsage {
                used: snapshot.live_api_keys,
                limit: limits::USER_API_KEYS_PER_ACCOUNT_MAX,
            },
        },
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
    fn usage_combines_tier_and_runtime_limits() {
        let space_id = Uuid::new_v4();
        let reconciled_at = Utc::now();
        let usage = build_usage(
            UserUsageSnapshot {
                tier: UserTier::Tier0,
                live_agents: 2,
                live_api_keys: 1,
                spaces: vec![SpaceUsageSnapshot {
                    id: space_id,
                    name: "Daily".to_owned(),
                    live_nodes: 7,
                    live_content_bytes: 512,
                    live_agent_connections: 2,
                    reconciled_at,
                    reconciliation_pending: true,
                }],
            },
            Limits {
                space_max_nodes: 5,
                space_max_content_bytes: 256,
                folder_max_children: 10,
            },
        );

        assert_eq!(usage.tier, UserTier::Tier0);
        assert_eq!(usage.account.spaces, QuotaUsage { used: 1, limit: 1 });
        assert_eq!(usage.account.agents, QuotaUsage { used: 2, limit: 3 });
        assert_eq!(usage.account.api_keys, QuotaUsage { used: 1, limit: 2 });
        assert_eq!(usage.spaces[0].id, space_id);
        assert_eq!(usage.spaces[0].nodes, QuotaUsage { used: 7, limit: 5 });
        assert_eq!(
            usage.spaces[0].content_bytes,
            QuotaUsage {
                used: 512,
                limit: 256,
            }
        );
        assert_eq!(
            usage.spaces[0].agent_connections,
            QuotaUsage { used: 2, limit: 5 }
        );
        assert!(usage.spaces[0].reconciliation.pending);
        assert_eq!(usage.spaces[0].reconciliation.reconciled_at, reconciled_at);
    }

    #[test]
    fn usage_rejects_agent_callers_before_repository_access() {
        assert!(matches!(
            require_user_caller(AccountKind::Agent),
            Err(ServiceError::Forbidden(_))
        ));
    }
}
