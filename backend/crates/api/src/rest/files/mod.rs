use axum::Router;
use axum::routing::{delete, get, patch, post};

use crate::state::AppState;

mod dto;
mod error;
pub(crate) mod handlers;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/files/root", get(handlers::root))
        .route("/v1/files/resolve", get(handlers::resolve))
        .route(
            "/v1/files/nodes/{node_id}/children",
            get(handlers::children),
        )
        .route("/v1/files/nodes/{node_id}/move", patch(handlers::move_node))
        .route("/v1/files/nodes/{node_id}", delete(handlers::delete_node))
        .route("/v1/files/folders", post(handlers::create_folder))
        .route("/v1/files/documents", post(handlers::create_document))
        .route(
            "/v1/files/documents/{node_id}",
            get(handlers::open_document).patch(handlers::save_document),
        )
        .route("/v1/files/search/find", post(handlers::find))
        .route("/v1/files/search/grep", post(handlers::grep))
}
