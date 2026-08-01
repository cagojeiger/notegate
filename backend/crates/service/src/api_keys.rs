//! Agent API-key issuance and shared token-format helpers.

use chrono::{Duration, Utc};
use notegate_core::limits;
use notegate_core::security::PiiCrypto;
use notegate_db::{ApiKeyRepo, api_key_repo::InsertApiKey};
use notegate_model::{ApiKeyCursor, ApiKeyPage, CreateApiKey, ListApiKeys, MintedApiKey};
use uuid::Uuid;

use crate::pagination::paginate_keyset;
use crate::{ServiceError, ServiceResult};

const AGENT_API_KEY_TOKEN_PREFIX: &str = "ngk_v2_";

pub async fn list_key_page(
    api_keys: &ApiKeyRepo,
    account_id: Uuid,
    request: ListApiKeys,
) -> ServiceResult<ApiKeyPage> {
    let (items, limit, has_more, next_cursor) = paginate_keyset(
        request.limit,
        limits::API_KEYS_DEFAULT_LIMIT,
        limits::API_KEYS_MAX_LIMIT,
        request.cursor.as_deref(),
        |limit, cursor: Option<ApiKeyCursor>| async move {
            Ok(api_keys
                .list_by_account(account_id, limit, cursor.as_ref())
                .await?)
        },
        |key| ApiKeyCursor {
            created_at: key.created_at,
            id: key.id,
        },
    )
    .await?;

    Ok(ApiKeyPage {
        items,
        limit,
        has_more,
        next_cursor,
    })
}

pub async fn create_agent_key(
    api_keys: &ApiKeyRepo,
    crypto: &PiiCrypto,
    agent_id: Uuid,
    created_by: Uuid,
    command: CreateApiKey,
) -> ServiceResult<MintedApiKey> {
    validate_key_command(&command)?;
    let key_id = Uuid::new_v4();
    let secret = generate_secret();
    let token = format_token(key_id, &secret);
    let token_hash = crypto.api_key_hash(&key_id.to_string(), &secret)?;
    let key = api_keys
        .insert_key_with_cap(
            InsertApiKey {
                key_id,
                account_id: agent_id,
                command: &command,
                token_prefix: &token_prefix(key_id),
                token_hash: &token_hash,
                created_by,
                rotated_from_key_id: None,
            },
            limits::AGENT_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await?;
    Ok(MintedApiKey { key, token })
}

pub async fn rotate_agent_key(
    api_keys: &ApiKeyRepo,
    crypto: &PiiCrypto,
    agent_id: Uuid,
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
                account_id: agent_id,
                command: &command,
                token_prefix: &token_prefix(key_id),
                token_hash: &token_hash,
                created_by,
                rotated_from_key_id: Some(old_key_id),
            },
            old_key_id,
            created_by,
            limits::AGENT_API_KEYS_PER_ACCOUNT_MAX,
        )
        .await?;
    Ok(MintedApiKey { key, token })
}

fn validate_key_command(command: &CreateApiKey) -> ServiceResult<()> {
    if command.name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "api key name cannot be empty".to_owned(),
        ));
    }
    if command.name.chars().count() > limits::API_KEY_NAME_MAX_CHARS {
        return Err(ServiceError::InvalidInput(format!(
            "api key name exceeds the maximum of {} characters",
            limits::API_KEY_NAME_MAX_CHARS
        )));
    }
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

    let max_expires_at = now + Duration::days(limits::AGENT_API_KEY_MAX_TTL_DAYS);
    if expires_at > max_expires_at {
        return Err(ServiceError::InvalidInput(format!(
            "api key expires_at must be within {} days",
            limits::AGENT_API_KEY_MAX_TTL_DAYS
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
    format!("{AGENT_API_KEY_TOKEN_PREFIX}{key_id}_{secret}")
}

pub fn token_prefix(key_id: Uuid) -> String {
    format!("{AGENT_API_KEY_TOKEN_PREFIX}{key_id}")
}

pub fn parse_token(token: &str) -> Option<(Uuid, &str)> {
    let rest = token.strip_prefix(AGENT_API_KEY_TOKEN_PREFIX)?;
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
        assert_eq!(token_prefix(key_id), format!("ngk_v2_{key_id}"));
    }

    #[test]
    fn api_key_expiry_is_required() {
        let command = CreateApiKey {
            name: "missing-expiry".to_owned(),
            scopes: Vec::new(),
            expires_at: None,
        };
        assert!(validate_key_command(&command).is_err());
    }

    #[test]
    fn api_key_name_must_be_non_empty_and_bounded() {
        let expires_at = Some(Utc::now() + Duration::days(1));
        let empty = CreateApiKey {
            name: "   ".to_owned(),
            scopes: Vec::new(),
            expires_at,
        };
        assert!(validate_key_command(&empty).is_err());

        let too_long = CreateApiKey {
            name: "k".repeat(limits::API_KEY_NAME_MAX_CHARS + 1),
            scopes: Vec::new(),
            expires_at,
        };
        assert!(validate_key_command(&too_long).is_err());
    }

    #[test]
    fn api_key_expiry_must_be_within_ttl() {
        let command = CreateApiKey {
            name: "too-long".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(Utc::now() + Duration::days(limits::AGENT_API_KEY_MAX_TTL_DAYS + 1)),
        };
        assert!(validate_key_command(&command).is_err());
    }

    #[test]
    fn api_key_expiry_accepts_future_within_ttl() {
        let command = CreateApiKey {
            name: "ok".to_owned(),
            scopes: Vec::new(),
            expires_at: Some(
                Utc::now() + Duration::days(limits::AGENT_API_KEY_MAX_TTL_DAYS)
                    - Duration::seconds(1),
            ),
        };
        assert!(validate_key_command(&command).is_ok());
    }

    #[test]
    fn api_key_token_rejects_invalid_and_legacy_formats() {
        assert!(parse_token("old-token").is_none());
        assert!(parse_token("ngk_v2_not-a-uuid_secret").is_none());
        assert!(parse_token("ngk_v2_00000000-0000-0000-0000-000000000000_").is_none());
        assert!(parse_token("ngk_v1_00000000-0000-0000-0000-000000000000_secret").is_none());
    }
}
