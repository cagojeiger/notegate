mod dto;

use axum::extract::Extension;
use axum::routing::get;
use axum::{Json, Router};
use notegate_model::Caller;

use crate::state::AppState;

use self::dto::MeResponse;

pub fn routes() -> Router<AppState> {
    Router::new().route("/me", get(get_me))
}

#[utoipa::path(
    get,
    path = "/api/v2/me",
    tag = "identity",
    responses((status = 200, description = "Get the Agent API-key caller", body = MeResponse)),
    security(("api_key" = []))
)]
pub(crate) async fn get_me(Extension(caller): Extension<Caller>) -> Json<MeResponse> {
    Json(MeResponse::from(&caller))
}
