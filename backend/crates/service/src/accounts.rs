//! Account lifecycle operations for the current caller.

use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, ApiKeyRepo};
use notegate_model::account::AccountKind;
use notegate_model::{ApiKeyPage, CreateApiKey, ListApiKeys, MintedApiKey};
use uuid::Uuid;

use crate::keys::{create_key_for_account, list_key_page, rotate_key_for_account};
use crate::{ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct AccountService {
    store: AccountRepo,
    api_keys: ApiKeyRepo,
    crypto: PiiCrypto,
}

impl AccountService {
    pub fn with_api_keys(store: AccountRepo, api_keys: ApiKeyRepo, crypto: PiiCrypto) -> Self {
        Self {
            store,
            api_keys,
            crypto,
        }
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

    pub async fn list_keys(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListApiKeys,
    ) -> ServiceResult<ApiKeyPage> {
        require_user(caller_kind)?;
        list_key_page(&self.api_keys, caller_account_id, request).await
    }

    pub async fn create_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        command: CreateApiKey,
    ) -> ServiceResult<MintedApiKey> {
        require_user(caller_kind)?;
        create_key_for_account(
            &self.api_keys,
            &self.crypto,
            caller_account_id,
            caller_account_id,
            command,
            None,
        )
        .await
    }

    pub async fn revoke_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        key_id: Uuid,
    ) -> ServiceResult<()> {
        require_user(caller_kind)?;
        Ok(self
            .api_keys
            .revoke_key(caller_account_id, key_id, caller_account_id, None)
            .await?)
    }

    pub async fn rotate_key(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        key_id: Uuid,
    ) -> ServiceResult<MintedApiKey> {
        require_user(caller_kind)?;
        let old = self
            .api_keys
            .find_live_key(caller_account_id, key_id)
            .await?
            .ok_or_else(|| ServiceError::NotFound("api key not found".to_owned()))?;
        rotate_key_for_account(
            &self.api_keys,
            &self.crypto,
            caller_account_id,
            caller_account_id,
            key_id,
            CreateApiKey {
                name: old.name,
                scopes: Vec::new(),
                expires_at: old.expires_at,
            },
        )
        .await
    }
}

fn require_user(kind: AccountKind) -> ServiceResult<()> {
    if kind == AccountKind::User {
        Ok(())
    } else {
        Err(ServiceError::Forbidden(
            "only user accounts may manage user API keys".to_owned(),
        ))
    }
}
