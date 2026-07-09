//! Identity category: `GET /api/v1/me` and `DELETE /api/v1/me`.
//!
//! `GET` returns the authenticated account, optional user/agent detail, and
//! global non-space capabilities via the shared [`build_me`] builder, kept
//! aligned with the MCP `me` tool (`docs/spec/mcp/identity.md`). Space-specific
//! permissions live in the Spaces category, not in `/me`.
//!
//! `DELETE` is the user account teardown endpoint. It is intentionally REST-only:
//! MCP remains a file/space tool surface and does not expose account deletion.

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use notegate_model::{Caller, CreateApiKey, ListApiKeys, ListAuditEvents};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;
use crate::identity::me::{MeOutput, build_me};
use crate::page::Page;
use crate::rest::dto::{
    ApiKeyMetadataListResponse, ApiKeyMetadataOut, AuditEventListResponse, AuditEventOut,
    CreateApiKeyBody,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/me", get(get_me).delete(delete_me))
        .route("/v1/me/keys", get(list_keys).post(create_key))
        .route("/v1/me/keys/{key_id}", post(rotate_key).delete(revoke_key))
        .route("/v1/me/audit-events", get(list_audit_events))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListKeysQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListEventsQuery {
    limit: Option<i64>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CreatedUserApiKeyOut {
    id: Uuid,
    account_id: Uuid,
    name: String,
    scopes: Vec<String>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    token: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/me",
    tag = "identity",
    responses((status = 200, description = "Get current caller", body = MeOutput)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn get_me(Extension(caller): Extension<Caller>) -> Json<MeOutput> {
    Json(build_me(&caller))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/keys",
    tag = "identity",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List current user API keys", body = ApiKeyMetadataListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list_keys(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListKeysQuery>,
) -> Result<Json<ApiKeyMetadataListResponse>, ApiError> {
    let page = state
        .account_lifecycle
        .list_keys(
            caller.account.kind,
            caller.account_id(),
            ListApiKeys {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let keys = page.items.iter().map(ApiKeyMetadataOut::from).collect();
    Ok(Json(ApiKeyMetadataListResponse {
        keys,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/audit-events",
    tag = "identity",
    params(
        ("limit" = Option<i64>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor"),
    ),
    responses((status = 200, description = "List current user audit event history", body = AuditEventListResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn list_audit_events(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<ListEventsQuery>,
) -> Result<Json<AuditEventListResponse>, ApiError> {
    let page = state
        .account_lifecycle
        .list_audit_events(
            caller.account.kind,
            caller.account_id(),
            ListAuditEvents {
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;
    let events = page.items.iter().map(AuditEventOut::from).collect();
    Ok(Json(AuditEventListResponse {
        events,
        page: Page::from_items(page.limit, &page.items, page.has_more, page.next_cursor),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/me/keys",
    tag = "identity",
    request_body = CreateApiKeyBody,
    responses((status = 201, description = "Create current user API key", body = CreatedUserApiKeyOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn create_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<CreateApiKeyBody>,
) -> Result<(StatusCode, Json<CreatedUserApiKeyOut>), ApiError> {
    let minted = state
        .account_lifecycle
        .create_key(
            caller.account.kind,
            caller.account_id(),
            CreateApiKey {
                name: body.name,
                scopes: body.scopes,
                expires_at: Some(body.expires_at),
            },
        )
        .await?;
    let key = minted.key;
    Ok((
        StatusCode::CREATED,
        Json(CreatedUserApiKeyOut {
            id: key.id,
            account_id: key.account_id,
            name: key.name,
            scopes: key.scopes,
            expires_at: key.expires_at,
            created_at: key.created_at,
            token: minted.token,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/me/keys/{key_id}",
    tag = "identity",
    params(("key_id" = Uuid, Path)),
    responses((status = 201, description = "Rotate current user API key", body = CreatedUserApiKeyOut)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn rotate_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(key_id): Path<Uuid>,
) -> Result<(StatusCode, Json<CreatedUserApiKeyOut>), ApiError> {
    let minted = state
        .account_lifecycle
        .rotate_key(caller.account.kind, caller.account_id(), key_id)
        .await?;
    let key = minted.key;
    Ok((
        StatusCode::CREATED,
        Json(CreatedUserApiKeyOut {
            id: key.id,
            account_id: key.account_id,
            name: key.name,
            scopes: key.scopes,
            expires_at: key.expires_at,
            created_at: key.created_at,
            token: minted.token,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/me/keys/{key_id}",
    tag = "identity",
    params(("key_id" = Uuid, Path)),
    responses((status = 204, description = "Revoke current user API key")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn revoke_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .account_lifecycle
        .revoke_key(caller.account.kind, caller.account_id(), key_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/v1/me",
    tag = "identity",
    responses((status = 204, description = "Delete current user account")),
    security(("bearer_auth" = []))
)]
pub(crate) async fn delete_me(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<StatusCode, ApiError> {
    state
        .account_lifecycle
        .delete_me(caller.account.kind, caller.account_id())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
