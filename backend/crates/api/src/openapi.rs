use axum::Router;
use notegate_core::Config;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::rest;
use crate::state::AppState;

/// The OpenAPI document is generated from `#[utoipa::path]` annotations on the
/// actual REST handlers, so route/method drift is caught close to the code that
/// serves the endpoint. `docs/spec/rest/` remains the REST contract;
/// `/openapi.json` is the machine-readable contract.
#[derive(OpenApi)]
#[openapi(
    paths(
        rest::me::get_me,
        rest::workspaces::list,
        rest::workspaces::create,
        rest::workspaces::get_one,
        rest::workspaces::rename,
        rest::workspaces::delete,
        rest::nodes::resolve_path,
        rest::nodes::create,
        rest::nodes::get_node,
        rest::nodes::update,
        rest::nodes::delete,
        rest::nodes::children,
        rest::nodes::move_node,
        rest::documents::read,
        rest::documents::replace,
        rest::documents::patch,
        rest::search::find,
        rest::search::grep,
        rest::access::list,
        rest::access::grant,
        rest::access::revoke,
        rest::agents::list,
        rest::agents::create,
        rest::agents::delete_agent,
        rest::agents::create_key,
        rest::agents::revoke_key,
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "identity", description = "Current caller identity"),
        (name = "workspaces", description = "Workspace lifecycle"),
        (name = "nodes", description = "Folder/document tree metadata"),
        (name = "documents", description = "Markdown content read/write/patch"),
        (name = "search", description = "find / grep within a workspace"),
        (name = "access", description = "Workspace role grant/revoke"),
        (name = "agents", description = "Agent account and key lifecycle"),
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

pub fn routes(config: &Config) -> Router<AppState> {
    if config.openapi_enabled {
        SwaggerUi::new("/swagger-ui")
            .url("/openapi.json", ApiDoc::openapi())
            .into()
    } else {
        Router::new()
    }
}

pub fn json_pretty() -> serde_json::Result<String> {
    serde_json::to_string_pretty(&ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_in_result
    )]
    use utoipa::OpenApi;

    use super::ApiDoc;

    #[test]
    fn openapi_defines_bearer_security_scheme() {
        let doc = ApiDoc::openapi();
        let components = doc.components.expect("components present");
        assert!(components.security_schemes.contains_key("bearer_auth"));
    }

    #[test]
    fn openapi_lists_every_rest_category() {
        let doc = ApiDoc::openapi();
        let paths = &doc.paths.paths;
        for path in [
            "/api/v1/me",
            "/api/v1/workspaces",
            "/api/v1/workspaces/{workspace_id}",
            "/api/v1/workspaces/{workspace_id}/paths/resolve",
            "/api/v1/workspaces/{workspace_id}/nodes",
            "/api/v1/workspaces/{workspace_id}/nodes/{node_id}",
            "/api/v1/workspaces/{workspace_id}/nodes/{node_id}/children",
            "/api/v1/workspaces/{workspace_id}/nodes/{node_id}/move",
            "/api/v1/workspaces/{workspace_id}/documents/{node_id}",
            "/api/v1/workspaces/{workspace_id}/search/find",
            "/api/v1/workspaces/{workspace_id}/search/grep",
            "/api/v1/workspaces/{workspace_id}/access",
            "/api/v1/workspaces/{workspace_id}/access/{account_id}",
            "/api/v1/agents",
            "/api/v1/agents/{agent_id}",
            "/api/v1/agents/{agent_id}/keys",
            "/api/v1/agents/{agent_id}/keys/{key_id}",
        ] {
            assert!(paths.contains_key(path), "missing OpenAPI path: {path}");
        }
    }
}
