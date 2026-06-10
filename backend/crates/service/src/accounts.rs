//! Account lifecycle operations for the current caller.

use chrono::Utc;
use notegate_core::limits;
use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::account::AccountKind;
use notegate_model::{ApiKeyCursor, ApiKeyPage, CreateApiKey, ListApiKeys, MintedApiKey};
use uuid::Uuid;

use crate::agents::{format_token, parse_token, token_prefix};
use crate::pagination::clamp_limit;
use crate::{ServiceError, ServiceResult, cursor};

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

pub async fn list_key_page(
    api_keys: &ApiKeyRepo,
    account_id: Uuid,
    request: ListApiKeys,
) -> ServiceResult<ApiKeyPage> {
    let limit = clamp_limit(
        request.limit,
        limits::API_KEYS_DEFAULT_LIMIT,
        limits::API_KEYS_MAX_LIMIT,
    );
    let cursor = match request.cursor.as_deref() {
        None => None,
        Some(raw) => Some(
            cursor::decode::<ApiKeyCursor>(raw)
                .map_err(|_error| ServiceError::InvalidInput("invalid cursor".to_owned()))?,
        ),
    };

    let mut items = api_keys
        .list_by_account(account_id, limit + 1, cursor.as_ref())
        .await?;
    let has_more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    let next_cursor = if has_more {
        items
            .last()
            .map(|key| ApiKeyCursor {
                created_at: key.created_at,
                id: key.id,
            })
            .map(|cursor| cursor::encode(&cursor))
            .transpose()
            .map_err(|_error| ServiceError::Internal("failed to encode cursor".to_owned()))?
    } else {
        None
    };

    Ok(ApiKeyPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}

pub async fn create_key_for_account(
    api_keys: &ApiKeyRepo,
    crypto: &PiiCrypto,
    account_id: Uuid,
    created_by: Uuid,
    command: CreateApiKey,
    rotated_from_key_id: Option<Uuid>,
) -> ServiceResult<MintedApiKey> {
    validate_key_command(&command)?;
    let key_id = Uuid::new_v4();
    let secret = generate_secret();
    let token = format_token(key_id, &secret);
    let token_hash = crypto.api_key_hash(&key_id.to_string(), &secret)?;
    let key = api_keys
        .insert_key_with_cap(InsertApiKey {
            key_id,
            account_id,
            command: &command,
            token_prefix: &token_prefix(key_id),
            token_hash: &token_hash,
            created_by,
            rotated_from_key_id,
        })
        .await?;
    Ok(MintedApiKey { key, token })
}

pub async fn rotate_key_for_account(
    api_keys: &ApiKeyRepo,
    crypto: &PiiCrypto,
    account_id: Uuid,
    created_by: Uuid,
    old_key_id: Uuid,
    command: CreateApiKey,
) -> ServiceResult<MintedApiKey> {
    validate_key_command(&command)?;

    let key_id = Uuid::new_v4();
    let secret = generate_secret();
    let token = format_token(key_id, &secret);
    let token_hash = crypto.api_key_hash(&key_id.to_string(), &secret)?;
    let key = api_keys
        .rotate_key(
            InsertApiKey {
                key_id,
                account_id,
                command: &command,
                token_prefix: &token_prefix(key_id),
                token_hash: &token_hash,
                created_by,
                rotated_from_key_id: Some(old_key_id),
            },
            old_key_id,
            created_by,
        )
        .await?;
    Ok(MintedApiKey { key, token })
}

fn validate_key_command(command: &CreateApiKey) -> ServiceResult<()> {
    if !command.scopes.is_empty() {
        return Err(ServiceError::InvalidInput(
            "api key scopes must be empty".to_owned(),
        ));
    }
    if command
        .expires_at
        .is_some_and(|expires_at| expires_at <= Utc::now())
    {
        return Err(ServiceError::InvalidInput(
            "api key expires_at must be in the future".to_owned(),
        ));
    }
    Ok(())
}

fn generate_secret() -> String {
    use rand::RngCore as _;
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[allow(dead_code)]
fn _assert_parse_is_visible(token: &str) -> Option<(Uuid, &str)> {
    parse_token(token)
}
