//! Thin HTTP boundary for the transport-neutral command engine.

mod context;
mod error;

use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::routing::{get, post};
use axum::{Json, Router};
use notegate_command::{
    FileDownloadInput, FileUploadInput, ManageInput, ReadInput, SearchInput, WriteInput,
};
use serde_json::Value;

use self::context::HttpCommandContext;
use self::error::CommandHttpError;
use crate::commands::{executor, identity, transfers};
use crate::state::AppState;

/// Routes relative to the `/api/commands/v1` mount point.
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .route("/read", post(read))
        .route("/search", post(search))
        .route("/write", post(write))
        .route("/manage", post(manage))
        .route("/file_upload", post(file_upload))
        .route("/file_download", post(file_download))
}

async fn me(context: HttpCommandContext) -> Json<identity::IdentityOutput> {
    Json(identity::call(context.as_command()))
}

async fn read(
    State(state): State<AppState>,
    context: HttpCommandContext,
    input: Result<Json<ReadInput>, JsonRejection>,
) -> Result<Json<Value>, CommandHttpError> {
    let input = json_input(input)?;
    executor::read(&state, context.as_command(), input)
        .await
        .map(Json)
        .map_err(Into::into)
}

async fn search(
    State(state): State<AppState>,
    context: HttpCommandContext,
    input: Result<Json<SearchInput>, JsonRejection>,
) -> Result<Json<Value>, CommandHttpError> {
    let input = json_input(input)?;
    executor::search(&state, context.as_command(), input)
        .await
        .map(Json)
        .map_err(Into::into)
}

async fn write(
    State(state): State<AppState>,
    context: HttpCommandContext,
    input: Result<Json<WriteInput>, JsonRejection>,
) -> Result<Json<Value>, CommandHttpError> {
    let input = json_input(input)?;
    executor::write(&state, context.as_command(), input)
        .await
        .map(Json)
        .map_err(Into::into)
}

async fn manage(
    State(state): State<AppState>,
    context: HttpCommandContext,
    input: Result<Json<ManageInput>, JsonRejection>,
) -> Result<Json<Value>, CommandHttpError> {
    let input = json_input(input)?;
    executor::manage(&state, context.as_command(), input)
        .await
        .map(Json)
        .map_err(Into::into)
}

async fn file_upload(
    State(state): State<AppState>,
    context: HttpCommandContext,
    input: Result<Json<FileUploadInput>, JsonRejection>,
) -> Result<Json<Value>, CommandHttpError> {
    let input = json_input(input)?;
    transfers::upload(&state, context.as_command(), input)
        .await
        .map(Json)
        .map_err(Into::into)
}

async fn file_download(
    State(state): State<AppState>,
    context: HttpCommandContext,
    input: Result<Json<FileDownloadInput>, JsonRejection>,
) -> Result<Json<Value>, CommandHttpError> {
    let input = json_input(input)?;
    transfers::download(&state, context.as_command(), input)
        .await
        .map(Json)
        .map_err(Into::into)
}

fn json_input<T>(input: Result<Json<T>, JsonRejection>) -> Result<T, CommandHttpError> {
    input
        .map(|Json(value)| value)
        .map_err(CommandHttpError::invalid_json)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]

    use axum::extract::Extension;
    use axum::http::StatusCode;
    use notegate_db::test_support::TestDb;
    use notegate_model::Channel;
    use serde_json::json;

    use super::*;
    use crate::rest::test_support::{caller_and_space, json_request, state};

    #[tokio::test]
    async fn read_route_uses_the_shared_engine_and_preserves_recovery_data()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = TestDb::setup().await? else {
            return Ok(());
        };
        let state = state(&db);
        let (mut caller, _space_id, _root_id) = caller_and_space(&state).await?;
        caller.channel = Channel::Api;
        let app = Router::new()
            .nest("/api/commands/v1", routes())
            .layer(Extension(caller))
            .with_state(state);

        let (status, spaces) = json_request(
            app.clone(),
            "POST",
            "/api/commands/v1/read".to_owned(),
            json!({
                "purpose": "list accessible spaces",
                "op": "spaces"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{spaces}");
        assert_eq!(spaces["spaces"][0]["name"], "rest-test");

        let (status, error) = json_request(
            app,
            "POST",
            "/api/commands/v1/read".to_owned(),
            json!({
                "purpose": "read a text node",
                "op": "read"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["error"], "required_field_missing");
        assert_eq!(error["kind"], "invalid_input");
        assert_eq!(error["data"]["code"], "required_field_missing");
        assert_eq!(error["data"]["next_action"]["kind"], "add_fields");
        assert_eq!(error["data"]["next_action"]["fields"][0]["field"], "target");

        db.cleanup().await;
        Ok(())
    }
}
