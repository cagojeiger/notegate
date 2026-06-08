use axum::Router;
use notegate_core::Config;
use utoipa::openapi::Ref;
use utoipa::openapi::content::Content;
use utoipa::openapi::path::{Operation, PathItem};
use utoipa::openapi::response::Response;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::rest;
use crate::state::AppState;

/// The OpenAPI document is the machine-readable contract for the `/api/v1`
/// JSON resource API only. Auth redirect/session endpoints, OAuth discovery
/// metadata, MCP, and system health/readiness endpoints are intentionally kept
/// outside this document; see `docs/spec/rest/README.md` for the scope decision.
///
/// The document is generated from `#[utoipa::path]` annotations on the actual
/// REST resource handlers, so route/method drift is caught close to the code
/// that serves each endpoint.
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
    components(schemas(crate::error::ErrorResponse)),
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

        add_default_error_response(openapi);
    }
}

fn add_default_error_response(openapi: &mut utoipa::openapi::OpenApi) {
    for item in openapi.paths.paths.values_mut() {
        for operation in operations_mut(item) {
            operation
                .responses
                .responses
                .entry("default".to_owned())
                .or_insert_with(|| error_response().into());
        }
    }
}

fn operations_mut(item: &mut PathItem) -> impl Iterator<Item = &mut Operation> {
    [
        item.get.as_mut(),
        item.put.as_mut(),
        item.post.as_mut(),
        item.delete.as_mut(),
        item.options.as_mut(),
        item.head.as_mut(),
        item.patch.as_mut(),
        item.trace.as_mut(),
    ]
    .into_iter()
    .flatten()
}

fn error_response() -> Response {
    let mut response = Response::new("Common REST error response");
    response.content.insert(
        "application/json".to_owned(),
        Content::new(Some(Ref::from_schema_name("ErrorResponse"))),
    );
    response
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
    fn openapi_uses_distinct_list_response_schemas() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");
        let schemas = value["components"]["schemas"]
            .as_object()
            .expect("schemas object");

        for schema in [
            "WorkspacesListResponse",
            "AccessListResponse",
            "AgentsListResponse",
            "ErrorResponse",
        ] {
            assert!(schemas.contains_key(schema), "missing schema: {schema}");
        }
        assert!(
            !schemas.contains_key("ListResponse"),
            "generic ListResponse schema should not collide across categories"
        );

        assert_eq!(
            response_ref(&value, "/api/v1/workspaces", "get", "200"),
            "#/components/schemas/WorkspacesListResponse"
        );
        assert_eq!(
            response_ref(
                &value,
                "/api/v1/workspaces/{workspace_id}/access",
                "get",
                "200"
            ),
            "#/components/schemas/AccessListResponse"
        );
        assert_eq!(
            response_ref(&value, "/api/v1/agents", "get", "200"),
            "#/components/schemas/AgentsListResponse"
        );
    }

    #[test]
    fn openapi_documents_rest_query_parameters() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");

        assert_query_params(&value, "/api/v1/workspaces", "get", &["limit", "cursor"]);
        assert_query_params(&value, "/api/v1/agents", "get", &["limit", "cursor"]);
        assert_query_params(
            &value,
            "/api/v1/workspaces/{workspace_id}/access",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/workspaces/{workspace_id}/paths/resolve",
            "get",
            &["path"],
        );
        assert_query_params(
            &value,
            "/api/v1/workspaces/{workspace_id}/nodes/{node_id}/children",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/workspaces/{workspace_id}/documents/{node_id}",
            "get",
            &[
                "start_line",
                "max_lines",
                "max_bytes",
                "if_none_match_sha256",
            ],
        );
        assert_query_params(
            &value,
            "/api/v1/workspaces/{workspace_id}/nodes/{node_id}",
            "delete",
            &["recursive"],
        );
    }

    #[test]
    fn openapi_adds_common_error_response_to_every_operation() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");
        let paths = value["paths"].as_object().expect("paths object");

        for (path, item) in paths {
            let item = item.as_object().expect("path item object");
            for (method, operation) in item {
                if !matches!(
                    method.as_str(),
                    "get" | "put" | "post" | "delete" | "patch" | "options" | "head" | "trace"
                ) {
                    continue;
                }
                let schema_ref = operation["responses"]["default"]["content"]["application/json"]
                    ["schema"]["$ref"]
                    .as_str()
                    .unwrap_or_default();
                assert_eq!(
                    schema_ref, "#/components/schemas/ErrorResponse",
                    "missing default ErrorResponse for {method} {path}"
                );
            }
        }
    }

    fn response_ref(value: &serde_json::Value, path: &str, method: &str, status: &str) -> String {
        value["paths"][path][method]["responses"][status]["content"]["application/json"]
            ["schema"]["$ref"]
            .as_str()
            .expect("response schema ref")
            .to_owned()
    }

    fn assert_query_params(value: &serde_json::Value, path: &str, method: &str, expected: &[&str]) {
        let parameters = value["paths"][path][method]["parameters"]
            .as_array()
            .expect("parameters array");
        for name in expected {
            assert!(
                parameters.iter().any(|param| {
                    param["name"] == *name && param["in"].as_str() == Some("query")
                }),
                "missing query parameter {name} for {method} {path}"
            );
        }
    }

    #[test]
    fn openapi_excludes_non_resource_api_surfaces() {
        let doc = ApiDoc::openapi();
        let paths = &doc.paths.paths;

        for path in [
            "/auth/login",
            "/auth/callback",
            "/auth/success",
            "/auth/logout",
            "/.well-known/oauth-authorization-server",
            "/.well-known/oauth-protected-resource",
            "/mcp",
            "/health",
            "/ready",
        ] {
            assert!(
                !paths.contains_key(path),
                "non-resource endpoint should stay outside OpenAPI: {path}"
            );
        }
    }

    #[test]
    fn openapi_lists_every_resource_api_category() {
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
