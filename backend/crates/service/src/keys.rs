//! API-key service helpers shared by user and agent accounts.

use chrono::{Duration, Utc};
use notegate_core::limits;
use notegate_core::security::PiiCrypto;
use notegate_db::{ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::{ApiKeyCursor, ApiKeyPage, CreateApiKey, ListApiKeys, MintedApiKey};
use uuid::Uuid;

use crate::pagination::clamp_limit;
use crate::{ServiceError, ServiceResult, cursor};

#[derive(Debug, Clone, Copy)]
pub struct KeyPolicy {
    pub max_live_keys: usize,
    pub max_ttl_days: i64,
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
    policy: KeyPolicy,
) -> ServiceResult<MintedApiKey> {
    validate_key_command(&command, policy.max_ttl_days)?;
    let key_id = Uuid::new_v4();
    let secret = generate_secret();
    let token = format_token(key_id, &secret);
    let token_hash = crypto.api_key_hash(&key_id.to_string(), &secret)?;
    let key = api_keys
        .insert_key_with_cap(
            InsertApiKey {
                key_id,
                account_id,
                command: &command,
                token_prefix: &token_prefix(key_id),
                token_hash: &token_hash,
                created_by,
                rotated_from_key_id,
            },
            policy.max_live_keys,
        )
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
    policy: KeyPolicy,
) -> ServiceResult<MintedApiKey> {
    validate_key_command(&command, policy.max_ttl_days)?;

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
            policy.max_live_keys,
        )
        .await?;
    Ok(MintedApiKey { key, token })
}

fn validate_key_command(command: &CreateApiKey, max_ttl_days: i64) -> ServiceResult<()> {
    if !command.scopes.is_empty() {
        return Err(ServiceError::InvalidInput(
            "api key scopes must be empty".to_owned(),
        ));
    }

    let now = Utc::now();
    let expires_at = command
        .expires_at
        .ok_or_else(|| ServiceError::InvalidInput("api key expires_at is required".to_owned()))?;
    if expires_at <= now {
        return Err(ServiceError::InvalidInput(
            "api key expires_at must be in the future".to_owned(),
        ));
    }

    let max_expires_at = now + Duration::days(max_ttl_days);
    if expires_at > max_expires_at {
        return Err(ServiceError::InvalidInput(format!(
            "api key expires_at must be within {max_ttl_days} days"
        )));
    }
    Ok(())
}

fn generate_secret() -> String {
    use rand::RngCore as _;
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn format_token(key_id: Uuid, secret: &str) -> String {
    format!("ngk_v1_{key_id}_{secret}")
}

pub fn token_prefix(key_id: Uuid) -> String {
    format!("ngk_v1_{key_id}")
}

pub fn parse_token(token: &str) -> Option<(Uuid, &str)> {
    let rest = token.strip_prefix("ngk_v1_")?;
    let (key_id, secret) = rest.split_once('_')?;
    let key_id = Uuid::parse_str(key_id).ok()?;
    if secret.is_empty() {
        return None;
    }
    Some((key_id, secret))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn api_key_token_round_trips_key_id_and_secret() {
        let key_id = Uuid::new_v4();
        let token = format_token(key_id, "secret-value");
        let parsed = parse_token(&token).unwrap();
        assert_eq!(parsed.0, key_id);
        assert_eq!(parsed.1, "secret-value");
        assert_eq!(token_prefix(key_id), format!("ngk_v1_{key_id}"));
    }

    #[test]
    fn api_key_expiry_is_required() {
        let command = CreateApiKey {
            name: "missing-expiry".to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        };
        assert!(validate_key_command(&command, 30).is_err());
    }

    #[test]
    fn api_key_expiry_must_be_within_ttl() {
        let command = CreateApiKey {
            name: "too-long".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(Utc::now() + Duration::days(31)),
        };
        assert!(validate_key_command(&command, 30).is_err());
    }

    #[test]
    fn api_key_expiry_accepts_future_within_ttl() {
        let command = CreateApiKey {
            name: "ok".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(Utc::now() + Duration::days(30) - Duration::seconds(1)),
        };
        assert!(validate_key_command(&command, 30).is_ok());
    }

    #[test]
    fn api_key_token_rejects_old_opaque_tokens() {
        assert!(parse_token("old-token").is_none());
        assert!(parse_token("ngk_v1_not-a-uuid_secret").is_none());
        assert!(parse_token("ngk_v1_00000000-0000-0000-0000-000000000000_").is_none());
    }
}
