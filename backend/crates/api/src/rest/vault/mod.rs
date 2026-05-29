use axum::Router;
use axum::routing::{delete, get, patch, post};

use crate::state::AppState;

mod dto;
mod error;
mod handlers;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/vault/root", get(handlers::root))
        .route("/v1/vault/resolve", get(handlers::resolve))
        .route(
            "/v1/vault/nodes/{node_id}/children",
            get(handlers::children),
        )
        .route("/v1/vault/nodes/{node_id}/move", patch(handlers::move_node))
        .route("/v1/vault/nodes/{node_id}", delete(handlers::delete_node))
        .route("/v1/vault/folders", post(handlers::create_folder))
        .route("/v1/vault/documents", post(handlers::create_document))
        .route(
            "/v1/vault/documents/{node_id}",
            get(handlers::open_document).patch(handlers::save_document),
        )
        .route("/v1/vault/search/find", post(handlers::find))
        .route("/v1/vault/search/grep", post(handlers::grep))
}
