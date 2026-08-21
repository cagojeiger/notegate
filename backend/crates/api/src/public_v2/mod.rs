mod dto;
pub(crate) mod files;
pub(crate) mod nodes;
pub(crate) mod spaces;
#[cfg(test)]
mod tests;
pub(crate) mod text;

use axum::extract::Extension;
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;

use crate::state::AppState;

use self::dto::MeResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me))
        .merge(spaces::routes())
        .merge(nodes::routes())
        .merge(text::routes())
        .merge(files::routes())
}

#[utoipa::path(
    get,
    path = "/api/v2/me",
    operation_id = "get_me",
    tag = "identity",
    responses((status = 200, description = "Get the Agent API-key caller", body = MeResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn get_me(Extension(caller): Extension<Caller>) -> Json<MeResponse> {
    Json(MeResponse::from(&caller))
}
