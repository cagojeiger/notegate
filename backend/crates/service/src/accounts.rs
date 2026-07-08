//! Account lifecycle operations for the current caller.

use notegate_core::{limits, security::PiiCrypto};
use notegate_db::{AccountRepo, ApiKeyRepo, AuditEventRepo};
use notegate_model::account::AccountKind;
use notegate_model::{
    ApiKeyPage, AuditEventPage, CreateApiKey, ListApiKeys, ListAuditEvents, MintedApiKey,
};
use uuid::Uuid;

use crate::api_keys::{
    ApiKeyPolicy, create_key_for_account, list_key_page, rotate_key_for_account,
};
use crate::audit_events::list_audit_event_page;
use crate::{ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct AccountService {
    store: AccountRepo,
    api_keys: ApiKeyRepo,
    audit_events: AuditEventRepo,
    crypto: PiiCrypto,
}

impl AccountService {
    pub fn with_api_keys(
        store: AccountRepo,
        api_keys: ApiKeyRepo,
        audit_events: AuditEventRepo,
        crypto: PiiCrypto,
    ) -> Self {
        Self {
            store,
            api_keys,
            audit_events,
            crypto,
        }
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

    pub async fn list_keys(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListApiKeys,
    ) -> ServiceResult<ApiKeyPage> {
        require_user(caller_kind)?;
        list_key_page(&self.api_keys, caller_account_id, request).await
    }

    /// List the caller's own audit event history (self-review). User callers only.
    pub async fn list_audit_events(
        &self,
        caller_kind: AccountKind,
        caller_account_id: Uuid,
        request: ListAuditEvents,
    ) -> ServiceResult<AuditEventPage> {
        require_user(caller_kind)?;
        list_audit_event_page(&self.audit_events, caller_account_id, request).await
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
            user_api_key_policy(),
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
                expires_at: Some(old.expires_at),
            },
            user_api_key_policy(),
        )
        .await
    }
}

fn user_api_key_policy() -> ApiKeyPolicy {
    ApiKeyPolicy {
        max_live_keys: limits::USER_API_KEYS_PER_ACCOUNT_MAX,
        max_ttl_days: limits::USER_API_KEY_MAX_TTL_DAYS,
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
