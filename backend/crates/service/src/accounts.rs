//! Account lifecycle operations for the current caller.

use notegate_db::AccountRepo;
use notegate_model::account::AccountKind;
use uuid::Uuid;

use crate::{ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct AccountService {
    store: AccountRepo,
}

impl AccountService {
    pub fn new(store: AccountRepo) -> Self {
        Self { store }
    }

    /// Soft-delete the current user account (ADR 0004). PII and the provider-sub
    /// tombstone are retained until the purge run anonymizes them after the retention
    /// window; re-login during that window is rejected, so a returning sub is never
    /// duplicated.
    ///
    /// Agent callers cannot delete accounts through this user lifecycle endpoint.
    pub async fn delete_me(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
    ) -> ServiceResult<()> {
        if caller_kind != AccountKind::User {
            return Err(ServiceError::Forbidden(
                "only user accounts may delete themselves".to_owned(),
            ));
        }
        // ADR 0004: spaces are cleaned up manually. Block deletion while the caller
        // still owns any live space — they must delete it first.
        let sole_owned = self
            .store
            .count_sole_owned_spaces(caller_account_id)
            .await?;
        if sole_owned > 0 {
            return Err(ServiceError::Conflict(format!(
                "delete your {sole_owned} owned space(s) before deleting your account"
            )));
        }
        Ok(self
            .store
            .soft_delete_user(caller_account_id, caller_account_id)
            .await?)
    }
}
