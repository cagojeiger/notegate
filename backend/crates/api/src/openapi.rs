use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use utoipa::openapi::Ref;
use utoipa::openapi::content::Content;
use utoipa::openapi::path::{Operation, PathItem};
use utoipa::openapi::response::Response;
#[cfg(test)]
use utoipa::openapi::security::{ApiKey, ApiKeyValue};
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::{mark_private_no_store, require_browser_session_for_docs};
#[cfg(test)]
use crate::rest;
use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "NoteGate REST API",
        description = "Stable resource API for Agent integrations. Authenticate with an Agent-owned ngk_v2_ bearer key. Access is limited to connected spaces and their read/write permissions."
    ),
    paths(
        crate::public_v2::get_me,
        crate::public_v2::spaces::list,
        crate::public_v2::spaces::get_one,
        crate::public_v2::nodes::resolve_path,
        crate::public_v2::nodes::tree,
        crate::public_v2::nodes::create,
        crate::public_v2::nodes::get_one,
        crate::public_v2::nodes::children,
        crate::public_v2::nodes::move_node,
        crate::public_v2::nodes::copy_node,
        crate::public_v2::nodes::delete,
        crate::public_v2::text::read,
        crate::public_v2::text::replace,
        crate::public_v2::text::patch,
        crate::public_v2::text::append,
        crate::public_v2::text::edit,
        crate::public_v2::search::find,
        crate::public_v2::search::grep,
        crate::public_v2::files::begin,
        crate::public_v2::files::parts,
        crate::public_v2::files::complete,
        crate::public_v2::files::abort,
        crate::public_v2::files::download,
    ),
    components(schemas(crate::error::ErrorResponse)),
    modifiers(&ApiKeySecurityAddon),
    tags(
        (name = "identity", description = "Agent API-key caller identity"),
        (name = "spaces", description = "Spaces connected to the Agent and their effective permissions"),
        (name = "nodes", description = "Folder, text, and file tree metadata and mutations"),
        (name = "text", description = "Bounded plain-text reads and optimistic-concurrency mutations"),
        (name = "search", description = "Bounded name, path, and plain-text search with opaque cursors"),
        (name = "files", description = "Single and multipart transfer through S3-compatible presigned URLs"),
    ),
    external_docs(
        url = "https://github.com/cagojeiger/notegate",
        description = "NoteGate source and specification"
    )
)]
pub struct PublicApiDoc;

/// Internal V1 handler catalog. The published OpenAPI contract is
/// [`PublicApiDoc`]; V1 is reserved for the first-party browser application.
#[cfg(test)]
#[derive(OpenApi)]
#[openapi(
    paths(
        rest::me::get_me,
        rest::me::get_usage,
        rest::me::list_audit_events,
        rest::me::list_mcp_invocations,
        rest::me::list_background_jobs,
        rest::me::get_background_job,
        rest::me::delete_me,
        rest::spaces::list,
        rest::spaces::create,
        rest::spaces::get_one,
        rest::spaces::update,
        rest::spaces::reorder,
        rest::spaces::delete,
        rest::spaces::request_usage_reconciliation,
        rest::nodes::resolve_path,
        rest::nodes::list,
        rest::nodes::list_file_change_events,
        rest::nodes::sync_file_changes,
        rest::nodes::create,
        rest::nodes::get_node,
        rest::nodes::reveal,
        rest::nodes::update,
        rest::nodes::update_search_policy,
        rest::nodes::update_write_lock,
        rest::nodes::delete,
        rest::nodes::children,
        rest::nodes::batch_children,
        rest::nodes::get_metadata,
        rest::nodes::move_node,
        rest::text::read,
        rest::text::replace,
        rest::text::patch,
        rest::text::update_encryption,
        rest::file_uploads::begin,
        rest::file_uploads::parts,
        rest::file_uploads::complete,
        rest::file_uploads::abort,
        rest::files::stat,
        rest::files::download,
        rest::files::preview_url,
        rest::files::pdf_preview_url,
        rest::files::docx_preview_url,
        rest::files::audio_preview_url,
        rest::files::batch_preview_urls,
        rest::connections::list,
        rest::connections::connect,
        rest::connections::disconnect,
        rest::agents::list,
        rest::agents::create,
        rest::agents::delete_agent,
        rest::agents::list_keys,
        rest::agents::create_key,
        rest::agents::rotate_key,
        rest::agents::revoke_key,
    ),
    components(schemas(crate::error::ErrorResponse)),
    modifiers(&BrowserSessionSecurityAddon),
    tags(
        (name = "identity", description = "Current caller identity"),
        (name = "events", description = "Audit and file change event history"),
        (name = "spaces", description = "Space lifecycle"),
        (name = "nodes", description = "Folder/text tree metadata"),
        (name = "text", description = "Text content read/write/patch"),
        (name = "files", description = "S3-compatible file upload/download"),
        (name = "connections", description = "Space agent connections"),
        (name = "agents", description = "Agent account and key lifecycle"),
    )
)]
pub struct ApiDoc;

#[cfg(test)]
struct BrowserSessionSecurityAddon;
struct ApiKeySecurityAddon;

#[cfg(test)]
impl Modify for BrowserSessionSecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "browser_session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                "notegate_browser_session",
                "Opaque browser session cookie",
            ))),
        );

        add_default_error_response(openapi);
    }
}

impl Modify for ApiKeySecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "api_key",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("NoteGate ngk_v2_ Agent API key")
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
    let mut response = Response::new(
        "Common JSON error. Inspect `error` for a stable code. Typical codes include invalid_input, forbidden, not_found, method_not_allowed, request_timeout, conflict, payload_too_large, node_write_locked, subtree_write_locked, search_busy, rate_limited, object_storage_unavailable, and internal_error. Retryable responses can include a Retry-After header.",
    );
    response.content.insert(
        "application/json".to_owned(),
        Content::new(Some(Ref::from_schema_name("ErrorResponse"))),
    );
    response
}

pub fn routes(state: AppState) -> Router<AppState> {
    let router: Router<AppState> = SwaggerUi::new("/swagger-ui/v2")
        .url("/openapi/v2.json", PublicApiDoc::openapi())
        .into();
    router
        .layer(from_fn_with_state(state, require_browser_session_for_docs))
        .layer(from_fn(mark_private_no_store))
}

pub fn json_pretty() -> serde_json::Result<String> {
    serde_json::to_string_pretty(&PublicApiDoc::openapi())
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

    use super::{ApiDoc, PublicApiDoc};

    #[test]
    fn public_openapi_contains_exact_agent_resource_contract() {
        let value =
            serde_json::to_value(PublicApiDoc::openapi()).expect("serializes public openapi");
        let paths = value["paths"].as_object().expect("paths object");
        let expected = [
            ("/api/v2/me", &["get"][..]),
            ("/api/v2/spaces", &["get"]),
            ("/api/v2/spaces/{space_id}", &["get"]),
            ("/api/v2/spaces/{space_id}/paths/resolve", &["get"]),
            ("/api/v2/spaces/{space_id}/tree", &["get"]),
            ("/api/v2/spaces/{space_id}/nodes", &["post"]),
            (
                "/api/v2/spaces/{space_id}/nodes/{node_id}",
                &["get", "delete"],
            ),
            (
                "/api/v2/spaces/{space_id}/nodes/{node_id}/children",
                &["get"],
            ),
            ("/api/v2/spaces/{space_id}/nodes/{node_id}/move", &["post"]),
            ("/api/v2/spaces/{space_id}/nodes/{node_id}/copy", &["post"]),
            (
                "/api/v2/spaces/{space_id}/text/{node_id}",
                &["get", "put", "patch"],
            ),
            ("/api/v2/spaces/{space_id}/text/{node_id}/append", &["post"]),
            ("/api/v2/spaces/{space_id}/text/{node_id}/edit", &["post"]),
            ("/api/v2/spaces/{space_id}/search/find", &["post"]),
            ("/api/v2/spaces/{space_id}/search/grep", &["post"]),
            ("/api/v2/spaces/{space_id}/file-uploads", &["post"]),
            (
                "/api/v2/spaces/{space_id}/file-uploads/{upload_id}/parts",
                &["post"],
            ),
            (
                "/api/v2/spaces/{space_id}/file-uploads/{upload_id}/complete",
                &["post"],
            ),
            (
                "/api/v2/spaces/{space_id}/file-uploads/{upload_id}",
                &["delete"],
            ),
            (
                "/api/v2/spaces/{space_id}/files/{node_id}/download",
                &["get"],
            ),
        ];

        assert_eq!(paths.len(), expected.len());
        for (path, methods) in expected {
            let operations = paths
                .get(path)
                .unwrap_or_else(|| panic!("missing public path: {path}"))
                .as_object()
                .expect("path operations");
            let actual_methods = operations
                .keys()
                .filter(|method| {
                    matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete")
                })
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            let expected_methods = methods
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                actual_methods, expected_methods,
                "method mismatch for {path}"
            );
            for method in methods {
                assert_eq!(
                    operations[*method]["security"][0]["api_key"],
                    serde_json::json!([]),
                    "{method} {path} must require the API-key scheme"
                );
            }
        }

        for forbidden in [
            "/api/v2/agents",
            "/api/v2/spaces/{space_id}/agents",
            "/api/v2/spaces/{space_id}/nodes/{node_id}/metadata",
            "/api/v2/spaces/{space_id}/nodes/{node_id}/write-lock",
        ] {
            assert!(
                !paths.contains_key(forbidden),
                "must not publish {forbidden}"
            );
        }
    }

    #[test]
    fn public_openapi_defines_api_key_security() {
        let value =
            serde_json::to_value(PublicApiDoc::openapi()).expect("serializes public openapi");
        let scheme = &value["components"]["securitySchemes"]["api_key"];
        assert_eq!(scheme["scheme"].as_str(), Some("bearer"));
        assert_eq!(
            scheme["bearerFormat"].as_str(),
            Some("NoteGate ngk_v2_ Agent API key")
        );
    }

    #[test]
    fn public_openapi_copy_conflict_uses_the_common_error_body() {
        let value =
            serde_json::to_value(PublicApiDoc::openapi()).expect("serializes public openapi");

        assert_eq!(
            response_ref(
                &value,
                "/api/v2/spaces/{space_id}/nodes/{node_id}/copy",
                "post",
                "409",
            ),
            "#/components/schemas/ErrorResponse"
        );
    }

    #[test]
    fn public_openapi_operation_ids_are_present_and_unique() {
        let value =
            serde_json::to_value(PublicApiDoc::openapi()).expect("serializes public openapi");
        let paths = value["paths"].as_object().expect("paths object");
        let mut operation_ids = std::collections::BTreeSet::new();

        for (path, item) in paths {
            let item = item.as_object().expect("path item object");
            for (method, operation) in item {
                if !matches!(
                    method.as_str(),
                    "get" | "put" | "post" | "delete" | "patch" | "options" | "head" | "trace"
                ) {
                    continue;
                }
                let operation_id = operation["operationId"]
                    .as_str()
                    .unwrap_or_else(|| panic!("missing operationId for {method} {path}"));
                assert!(
                    operation_ids.insert(operation_id),
                    "duplicate operationId {operation_id} for {method} {path}"
                );
            }
        }
    }

    #[test]
    fn public_openapi_describes_external_client_contract() {
        let value =
            serde_json::to_value(PublicApiDoc::openapi()).expect("serializes public openapi");
        let schemas = &value["components"]["schemas"];

        assert_eq!(value["info"]["title"], "NoteGate REST API");
        assert!(
            value["info"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("ngk_v2_"))
        );
        assert_eq!(
            value["externalDocs"]["url"],
            "https://github.com/cagojeiger/notegate"
        );
        assert_eq!(
            schemas["FindBody"]["properties"]["match"]["default"],
            "contains"
        );
        assert_eq!(
            schemas["GrepBody"]["properties"]["lines"]["default"],
            "none"
        );
        assert_eq!(
            schemas["PatchEditBody"]["properties"]["mode"]["default"],
            "unique"
        );
        assert_eq!(
            schemas["BeginUploadBody"]["properties"]["encryption_mode"]["default"],
            "none"
        );
        for (schema, values) in [
            ("AccountKindOut", serde_json::json!(["user", "agent"])),
            ("PermissionOut", serde_json::json!(["read", "write"])),
            ("NodeKindOut", serde_json::json!(["folder", "text", "file"])),
            (
                "TextStorageFormatOut",
                serde_json::json!(["plain", "encrypted"]),
            ),
            (
                "TextAtRestEncryptionOut",
                serde_json::json!(["none", "server"]),
            ),
            (
                "FileEncryptionModeOut",
                serde_json::json!(["none", "client"]),
            ),
            ("CreateNodeKind", serde_json::json!(["folder", "text"])),
            (
                "SearchNodeKind",
                serde_json::json!(["folder", "text", "file"]),
            ),
            (
                "FindMatch",
                serde_json::json!(["contains", "regex", "glob"]),
            ),
            ("GrepMatch", serde_json::json!(["literal", "regex"])),
            ("GrepLines", serde_json::json!(["none", "first", "all"])),
            (
                "PatchMatchMode",
                serde_json::json!(["unique", "first", "all"]),
            ),
            (
                "LineEditOperation",
                serde_json::json!([
                    "insert_before_line",
                    "insert_after_line",
                    "replace_lines",
                    "delete_lines"
                ]),
            ),
            (
                "UploadEncryptionMode",
                serde_json::json!(["none", "client"]),
            ),
        ] {
            assert_eq!(
                schemas[schema]["enum"], values,
                "enum mismatch for {schema}"
            );
        }
        assert!(
            schemas["PageOut"]["properties"]["next_cursor"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("Opaque"))
        );
    }

    #[test]
    fn openapi_defines_browser_session_security_scheme() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");
        let scheme = &value["components"]["securitySchemes"]["browser_session"];
        assert_eq!(scheme["type"].as_str(), Some("apiKey"));
        assert_eq!(scheme["in"].as_str(), Some("cookie"));
        assert_eq!(scheme["name"].as_str(), Some("notegate_browser_session"));
    }

    #[test]
    fn openapi_api_key_create_requires_expires_at() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");
        let required = value["components"]["schemas"]["CreateApiKeyBody"]["required"]
            .as_array()
            .expect("required array");

        for field in ["name", "expires_at"] {
            assert!(
                required.iter().any(|value| value.as_str() == Some(field)),
                "CreateApiKeyBody should require {field}"
            );
        }
    }

    #[test]
    fn openapi_uses_distinct_list_response_schemas() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");
        let schemas = value["components"]["schemas"]
            .as_object()
            .expect("schemas object");

        for schema in [
            "SpacesListResponse",
            "NodesListResponse",
            "RevealResponse",
            "ConnectionListResponse",
            "AgentsListResponse",
            "BackgroundJobListResponse",
            "BackgroundJobDetailResponse",
            "ErrorResponse",
        ] {
            assert!(schemas.contains_key(schema), "missing schema: {schema}");
        }
        assert!(
            !schemas.contains_key("ListResponse"),
            "generic ListResponse schema should not collide across categories"
        );

        assert_eq!(
            response_ref(&value, "/api/v1/spaces", "get", "200"),
            "#/components/schemas/SpacesListResponse"
        );
        assert_eq!(
            response_ref(&value, "/api/v1/spaces/{space_id}/nodes", "get", "200"),
            "#/components/schemas/NodesListResponse"
        );
        assert_eq!(
            response_ref(
                &value,
                "/api/v1/spaces/{space_id}/nodes/{node_id}/reveal",
                "get",
                "200"
            ),
            "#/components/schemas/RevealResponse"
        );
        assert_eq!(
            response_ref(&value, "/api/v1/spaces/{space_id}/agents", "get", "200"),
            "#/components/schemas/ConnectionListResponse"
        );
        assert_eq!(
            response_ref(&value, "/api/v1/agents", "get", "200"),
            "#/components/schemas/AgentsListResponse"
        );
        assert_eq!(
            response_ref(&value, "/api/v1/me/jobs", "get", "200"),
            "#/components/schemas/BackgroundJobListResponse"
        );
        assert_eq!(
            response_ref(&value, "/api/v1/me/jobs/{job_id}", "get", "200"),
            "#/components/schemas/BackgroundJobDetailResponse"
        );
    }

    #[test]
    fn openapi_includes_usage_routes() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");

        assert_eq!(
            response_ref(&value, "/api/v1/me/usage", "get", "200"),
            "#/components/schemas/CurrentUserUsageOut"
        );
        assert_eq!(
            response_ref(
                &value,
                "/api/v1/spaces/{space_id}/usage/reconcile",
                "post",
                "202"
            ),
            "#/components/schemas/ReconciliationQueuedResponse"
        );
        assert_eq!(
            response_ref(
                &value,
                "/api/v1/spaces/{space_id}/usage/reconcile",
                "post",
                "409"
            ),
            "#/components/schemas/ErrorResponse"
        );
        assert_eq!(
            response_ref(
                &value,
                "/api/v1/spaces/{space_id}/usage/reconcile",
                "post",
                "503"
            ),
            "#/components/schemas/ErrorResponse"
        );
    }

    #[test]
    fn openapi_texts_rest_query_parameters() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");

        assert_query_params(&value, "/api/v1/spaces", "get", &["limit", "cursor"]);
        assert_query_params(&value, "/api/v1/agents", "get", &["limit", "cursor"]);
        assert_query_params(
            &value,
            "/api/v1/me/audit-events",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/me/mcp-invocations",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/agents/{agent_id}/keys",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/agents",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/paths/resolve",
            "get",
            &["path"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/nodes",
            "get",
            &["kind", "sort", "limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/file-change-events",
            "get",
            &["node_id", "limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/file-change-sync",
            "get",
            &["after_id", "limit"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/nodes/{node_id}/children",
            "get",
            &["limit", "cursor"],
        );
        assert_query_params(
            &value,
            "/api/v1/spaces/{space_id}/text/{node_id}",
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
            "/api/v1/spaces/{space_id}/nodes/{node_id}",
            "delete",
            &["recursive"],
        );
    }

    #[test]
    fn openapi_spaces_schema_matches_update_contract() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");

        assert_eq!(
            value["components"]["schemas"]["SpaceOut"]["properties"]["sort_order"]["type"],
            "integer"
        );
        assert_eq!(
            value["components"]["schemas"]["SpaceOut"]["properties"]["navigation_pinned"]["type"],
            "boolean"
        );
        assert_eq!(
            value["components"]["schemas"]["SpaceOut"]["properties"]["user_mcp_enabled"]["type"],
            "boolean"
        );
        assert_eq!(
            value["paths"]["/api/v1/spaces/{space_id}"]["patch"]["requestBody"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/UpdateBody"
        );
        assert!(
            value["components"]["schemas"]["UpdateBody"]["properties"]
                .as_object()
                .expect("UpdateBody properties")
                .contains_key("sort_order"),
            "UpdateBody must expose sort_order"
        );
        assert!(
            value["components"]["schemas"]["UpdateBody"]["properties"]
                .as_object()
                .expect("UpdateBody properties")
                .contains_key("navigation_pinned"),
            "UpdateBody must expose navigation_pinned"
        );
        assert!(
            value["components"]["schemas"]["UpdateBody"]["properties"]
                .as_object()
                .expect("UpdateBody properties")
                .contains_key("user_mcp_enabled"),
            "UpdateBody must expose user_mcp_enabled"
        );
        assert_eq!(
            value["paths"]["/api/v1/spaces:reorder"]["post"]["requestBody"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/ReorderBody"
        );
    }

    #[test]
    fn openapi_texts_connection_permission_enum() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");

        assert_eq!(
            value["components"]["schemas"]["PermissionBody"]["enum"],
            serde_json::json!(["read", "write"])
        );
        assert_eq!(
            value["components"]["schemas"]["ConnectBody"]["properties"]["permission"]["$ref"],
            "#/components/schemas/PermissionBody"
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
            "/auth/login-complete.js",
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
            "/api/v1/me/audit-events",
            "/api/v1/me/mcp-invocations",
            "/api/v1/spaces",
            "/api/v1/spaces:reorder",
            "/api/v1/spaces/{space_id}",
            "/api/v1/spaces/{space_id}/paths/resolve",
            "/api/v1/spaces/{space_id}/nodes",
            "/api/v1/spaces/{space_id}/file-change-events",
            "/api/v1/spaces/{space_id}/file-change-sync",
            "/api/v1/spaces/{space_id}/nodes/{node_id}",
            "/api/v1/spaces/{space_id}/nodes/{node_id}/children",
            "/api/v1/spaces/{space_id}/nodes/{node_id}/reveal",
            "/api/v1/spaces/{space_id}/nodes/{node_id}/search-policy",
            "/api/v1/spaces/{space_id}/nodes/{node_id}/write-lock",
            "/api/v1/spaces/{space_id}/nodes/{node_id}/move",
            "/api/v1/spaces/{space_id}/text/{node_id}",
            "/api/v1/spaces/{space_id}/text/{node_id}/encryption",
            "/api/v1/spaces/{space_id}/files/{node_id}",
            "/api/v1/spaces/{space_id}/files/{node_id}/content",
            "/api/v1/spaces/{space_id}/files/{node_id}/audio-preview-url",
            "/api/v1/spaces/{space_id}/files/{node_id}/docx-preview-url",
            "/api/v1/spaces/{space_id}/files/{node_id}/preview-url",
            "/api/v1/spaces/{space_id}/file-previews:batchResolve",
            "/api/v1/spaces/{space_id}/agents",
            "/api/v1/spaces/{space_id}/agents/{agent_id}",
            "/api/v1/agents",
            "/api/v1/agents/{agent_id}",
            "/api/v1/agents/{agent_id}/keys",
            "/api/v1/agents/{agent_id}/keys/{key_id}",
        ] {
            assert!(paths.contains_key(path), "missing OpenAPI path: {path}");
        }
    }

    #[test]
    fn openapi_lists_exact_resource_methods() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(doc).expect("serializes openapi");
        let paths = value["paths"].as_object().expect("paths object");

        let mut actual = Vec::new();
        for (path, item) in paths {
            let item = item.as_object().expect("path item object");
            for method in item.keys() {
                if matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete") {
                    actual.push(format!("{} {path}", method.to_uppercase()));
                }
            }
        }
        actual.sort();

        let mut expected = vec![
            "DELETE /api/v1/agents/{agent_id}",
            "DELETE /api/v1/agents/{agent_id}/keys/{key_id}",
            "DELETE /api/v1/spaces/{space_id}",
            "DELETE /api/v1/spaces/{space_id}/agents/{agent_id}",
            "DELETE /api/v1/spaces/{space_id}/file-uploads/{upload_id}",
            "DELETE /api/v1/spaces/{space_id}/nodes/{node_id}",
            "GET /api/v1/agents",
            "GET /api/v1/agents/{agent_id}/keys",
            "DELETE /api/v1/me",
            "GET /api/v1/me",
            "GET /api/v1/me/audit-events",
            "GET /api/v1/me/jobs",
            "GET /api/v1/me/jobs/{job_id}",
            "GET /api/v1/me/mcp-invocations",
            "GET /api/v1/me/usage",
            "GET /api/v1/spaces",
            "GET /api/v1/spaces/{space_id}",
            "GET /api/v1/spaces/{space_id}/agents",
            "GET /api/v1/spaces/{space_id}/text/{node_id}",
            "GET /api/v1/spaces/{space_id}/files/{node_id}",
            "GET /api/v1/spaces/{space_id}/files/{node_id}/audio-preview-url",
            "GET /api/v1/spaces/{space_id}/files/{node_id}/content",
            "GET /api/v1/spaces/{space_id}/files/{node_id}/docx-preview-url",
            "GET /api/v1/spaces/{space_id}/files/{node_id}/pdf-preview-url",
            "GET /api/v1/spaces/{space_id}/files/{node_id}/preview-url",
            "GET /api/v1/spaces/{space_id}/file-change-events",
            "GET /api/v1/spaces/{space_id}/file-change-sync",
            "GET /api/v1/spaces/{space_id}/nodes",
            "GET /api/v1/spaces/{space_id}/nodes/{node_id}",
            "GET /api/v1/spaces/{space_id}/nodes/{node_id}/children",
            "GET /api/v1/spaces/{space_id}/nodes/{node_id}/metadata",
            "GET /api/v1/spaces/{space_id}/nodes/{node_id}/reveal",
            "GET /api/v1/spaces/{space_id}/paths/resolve",
            "PATCH /api/v1/spaces/{space_id}",
            "PATCH /api/v1/spaces/{space_id}/text/{node_id}",
            "PATCH /api/v1/spaces/{space_id}/nodes/{node_id}",
            "POST /api/v1/agents",
            "POST /api/v1/agents/{agent_id}/keys",
            "POST /api/v1/agents/{agent_id}/keys/{key_id}",
            "POST /api/v1/spaces",
            "POST /api/v1/spaces:reorder",
            "POST /api/v1/spaces/{space_id}/file-uploads",
            "POST /api/v1/spaces/{space_id}/file-previews:batchResolve",
            "POST /api/v1/spaces/{space_id}/file-uploads/{upload_id}/complete",
            "POST /api/v1/spaces/{space_id}/file-uploads/{upload_id}/parts",
            "POST /api/v1/spaces/{space_id}/nodes",
            "POST /api/v1/spaces/{space_id}/nodes/{node_id}/move",
            "POST /api/v1/spaces/{space_id}/nodes:batchListChildren",
            "POST /api/v1/spaces/{space_id}/usage/reconcile",
            "PUT /api/v1/spaces/{space_id}/agents/{agent_id}",
            "PUT /api/v1/spaces/{space_id}/text/{node_id}",
            "PUT /api/v1/spaces/{space_id}/nodes/{node_id}/search-policy",
            "PUT /api/v1/spaces/{space_id}/nodes/{node_id}/write-lock",
            "PUT /api/v1/spaces/{space_id}/text/{node_id}/encryption",
        ];
        expected.sort();

        assert_eq!(actual, expected);
    }

    #[test]
    fn openapi_marks_every_v1_operation_as_browser_session_secured() {
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
                let security = operation["security"]
                    .as_array()
                    .expect("security requirement array");
                assert!(
                    security
                        .iter()
                        .any(|requirement| requirement.get("browser_session").is_some()),
                    "missing browser_session for {method} {path}"
                );
            }
        }
    }
}
