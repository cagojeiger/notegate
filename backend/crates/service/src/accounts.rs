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

    /// Deactivate the current user account and anonymize its PII.
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
        Ok(self
            .store
            .anonymize_user(caller_account_id, caller_account_id)
            .await?)
    }
}
