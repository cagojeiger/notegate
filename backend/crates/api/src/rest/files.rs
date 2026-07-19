//! File category: small inline binary upload/download.
//!
//! REST handles file content. MCP only exposes file nodes and metadata through
//! `ls`/`stat`/`find`.

use axum::body::Body;
use axum::extract::{Extension, Multipart, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderName, HeaderValue, LOCATION};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use notegate_model::{Caller, FileEncryptionMode};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use notegate_service::files::CreateFile;

use crate::error::ApiError;
use crate::rest::dto::{NodeOut, attribution_ids};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/{space_id}/files", post(upload))
        .route("/v1/spaces/{space_id}/files/{node_id}", get(stat))
        .route(
            "/v1/spaces/{space_id}/files/{node_id}/content",
            get(download),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileResponse {
    pub(crate) node: NodeOut,
}

#[utoipa::path(
    post,
    path = "/api/v1/spaces/{space_id}/files",
    tag = "files",
    params(("space_id" = Uuid, Path, description = "Space id")),
    responses((status = 201, description = "Uploaded file", body = FileResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn upload(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(space_id): Path<Uuid>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<FileResponse>), ApiError> {
    let input = parse_upload(multipart).await?;
    let view = state
        .files
        .create_file(
            caller.account_id(),
            space_id,
            CreateFile {
                parent_node_id: input.parent_node_id,
                name: input.name,
                bytes: input.bytes,
                media_type: input.media_type,
                original_filename: input.original_filename,
                encryption_mode: input.encryption_mode,
                encryption_metadata: input.encryption_metadata,
            },
        )
        .await?;
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view.node]))
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(FileResponse {
            node: NodeOut::from_view(&view.node, &refs),
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses((status = 200, description = "File metadata", body = FileResponse)),
    security(("bearer_auth" = []))
)]
pub(crate) async fn stat(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<FileResponse>, ApiError> {
    let view = state
        .files
        .stat(caller.account_id(), space_id, node_id)
        .await?;
    if view.node.kind != notegate_model::NodeKind::File {
        return Err(ApiError::invalid_field("target is not a file"));
    }
    let refs = state
        .accounts
        .find_account_refs(&attribution_ids([&view]))
        .await?;
    Ok(Json(FileResponse {
        node: NodeOut::from_view(&view, &refs),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/spaces/{space_id}/files/{node_id}/content",
    tag = "files",
    params(("space_id" = Uuid, Path), ("node_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Inline file content bytes"),
        (status = 302, description = "Redirect to a presigned object download URL")
    ),
    security(("bearer_auth" = []))
)]
pub(crate) async fn download(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((space_id, node_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let file_view = state
        .files
        .file_for_download(caller.account_id(), space_id, node_id)
        .await?;
    if file_view.file.storage_kind == notegate_model::FileStorageKind::Object {
        let object_key = file_view
            .file
            .object_key
            .as_deref()
            .ok_or_else(|| ApiError::internal("object file is missing its storage key"))?;
        let storage = state
            .object_storage
            .as_ref()
            .ok_or_else(ApiError::object_storage_unavailable)?;
        let get_url = storage
            .presign_get(object_key, file_view.file.original_filename.as_deref())
            .await?;
        let location =
            HeaderValue::from_str(&get_url).map_err(|_| ApiError::object_storage_unavailable())?;
        return Ok((StatusCode::FOUND, [(LOCATION, location)]).into_response());
    }

    let result = state
        .files
        .read_file(caller.account_id(), space_id, node_id)
        .await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&result.file.media_type)
            .map_err(|_| ApiError::internal("invalid stored media type"))?,
    );
    headers.insert(
        HeaderName::from_static("x-content-sha256"),
        HeaderValue::from_str(
            result
                .file
                .content_sha256
                .as_deref()
                .ok_or_else(|| ApiError::internal("inline file is missing its content hash"))?,
        )
        .map_err(|_| ApiError::internal("invalid stored content hash"))?,
    );
    headers.insert(
        HeaderName::from_static("x-encryption-mode"),
        HeaderValue::from_static(result.file.encryption_mode.as_str()),
    );
    if let Some(filename) = result.file.original_filename.as_deref() {
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!(
                "attachment; filename=\"{}\"",
                safe_filename(filename)
            ))
            .map_err(|_| ApiError::internal("invalid stored filename"))?,
        );
    }

    Ok((headers, Body::from(result.bytes)).into_response())
}

#[derive(Debug)]
struct UploadInput {
    parent_node_id: Uuid,
    name: String,
    bytes: Vec<u8>,
    media_type: String,
    original_filename: Option<String>,
    encryption_mode: FileEncryptionMode,
    encryption_metadata: Option<Value>,
}

async fn parse_upload(mut multipart: Multipart) -> Result<UploadInput, ApiError> {
    let mut parent_node_id = None;
    let mut name = None;
    let mut bytes = None;
    let mut media_type = None;
    let mut original_filename = None;
    let mut encryption_mode = FileEncryptionMode::None;
    let mut encryption_metadata = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::invalid_field(error.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_owned();
        if field_name == "file" {
            if original_filename.is_none() {
                original_filename = field.file_name().map(ToOwned::to_owned);
            }
            if media_type.is_none() {
                media_type = field.content_type().map(ToString::to_string);
            }
            let data = field
                .bytes()
                .await
                .map_err(|error| ApiError::invalid_field(error.to_string()))?;
            bytes = Some(data.to_vec());
            continue;
        }

        let value = field_text(field).await?;
        match field_name.as_str() {
            "parent_node_id" => {
                parent_node_id = Some(
                    Uuid::parse_str(&value)
                        .map_err(|_| ApiError::invalid_field("parent_node_id must be a UUID"))?,
                );
            }
            "name" => name = Some(value),
            "media_type" => media_type = Some(value),
            "original_filename" => original_filename = Some(value),
            "encryption_mode" => {
                encryption_mode = FileEncryptionMode::parse(&value).ok_or_else(|| {
                    ApiError::invalid_field("encryption_mode must be 'none' or 'client'")
                })?;
            }
            "encryption_metadata" => {
                let metadata: Value = serde_json::from_str(&value)
                    .map_err(|_| ApiError::invalid_field("encryption_metadata must be JSON"))?;
                encryption_metadata = Some(metadata);
            }
            _ => {
                return Err(ApiError::invalid_field(format!(
                    "unknown field '{field_name}'"
                )));
            }
        }
    }

    Ok(UploadInput {
        parent_node_id: parent_node_id
            .ok_or_else(|| ApiError::invalid_field("parent_node_id is required"))?,
        name: name.ok_or_else(|| ApiError::invalid_field("name is required"))?,
        bytes: bytes.ok_or_else(|| ApiError::invalid_field("file is required"))?,
        media_type: media_type.unwrap_or_else(|| "application/octet-stream".to_owned()),
        original_filename,
        encryption_mode,
        encryption_metadata,
    })
}

async fn field_text(field: axum::extract::multipart::Field<'_>) -> Result<String, ApiError> {
    let bytes = field
        .bytes()
        .await
        .map_err(|error| ApiError::invalid_field(error.to_string()))?;
    String::from_utf8(bytes.to_vec()).map_err(|_| ApiError::invalid_field("field must be UTF-8"))
}

fn safe_filename(filename: &str) -> String {
    filename
        .chars()
        .filter(|ch| !matches!(ch, '\\' | '/' | '"' | '\r' | '\n'))
        .collect()
}
