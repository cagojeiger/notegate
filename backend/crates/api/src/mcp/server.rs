//! rmcp 1.7.0 A1 adapter decision:
//! - Streamable HTTP server is `rmcp::transport::streamable_http_server::StreamableHttpService`.
//! - Axum integration is via the tower `Service`/`handle` API; this module wraps it in an axum
//!   handler so Bearer verification can run before rmcp consumes the body.
//! - rmcp injects raw `http::request::Parts` into each request's MCP extensions. We insert the
//!   verified domain `Caller` into the HTTP parts' `extensions` before calling rmcp; the `me` tool
//!   reads that request-scoped `Caller` through `Extension<Parts>`.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::header::WWW_AUTHENTICATE;
use axum::http::request::Parts;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, JsonObject, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, Json, ServerHandler, tool, tool_handler, tool_router};
use serde_json::Value;
use url::Url;

use notegate_model::Channel;

use crate::auth::api_key::verify_api_key;
use crate::auth::bearer::{
    AuthError, auth_error_body, extract_bearer, shared_scoped_challenge_header, status_for_error,
    verify_bearer_mcp,
};
use crate::identity::me::MeOutput;
use crate::mcp::tools;
use crate::state::AppState;

const MCP_SERVER_INSTRUCTIONS: &str = "Path-first workspace, file, and search tools for notegate.";

/// A permissive `{"type":"object"}` output schema for the path-first file tools.
///
/// Those tools return dynamic JSON objects (`Json<Value>`); rmcp 1.7 cannot
/// derive a valid MCP `outputSchema` from `serde_json::Value` (the spec requires
/// the schema root to be `type: object`, and `Value`'s schema has no root type),
/// and it panics at tool-list/call time if we let it try. Supplying this
/// object-typed schema satisfies the spec while keeping the concrete fields
/// dynamic. The typed `me` tool keeps its derived schema.
fn object_output_schema() -> Arc<JsonObject> {
    let mut schema = JsonObject::new();
    schema.insert("type".to_owned(), Value::String("object".to_owned()));
    Arc::new(schema)
}

/// The MCP server handler. Holds a clone of the shared [`AppState`] so each
/// path-first tool can call the same services REST uses; the authenticated
/// [`Caller`](notegate_model::Caller) is read per-request from the HTTP
/// `Parts` the auth wrapper inserts.
#[derive(Clone)]
pub struct McpServer {
    state: AppState,
}

#[tool_router]
impl McpServer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    #[tool(name = "me", description = "Return the authenticated caller identity.")]
    pub async fn me_tool(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<Json<MeOutput>, ErrorData> {
        tools::identity::call(&parts)
    }

    #[tool(
        name = "workspaces_list",
        description = "List workspaces accessible to the authenticated caller.",
        output_schema = object_output_schema()
    )]
    pub async fn workspaces_list_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::workspaces::ListInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::workspaces::list(&self.state, &parts, params).await
    }

    #[tool(
        name = "workspaces_create",
        description = "Create a workspace owned by the authenticated user caller.",
        output_schema = object_output_schema()
    )]
    pub async fn workspaces_create_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::workspaces::CreateInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::workspaces::create(&self.state, &parts, params).await
    }

    #[tool(
        name = "workspaces_get",
        description = "Return one accessible workspace by name, id, or single-workspace fallback.",
        output_schema = object_output_schema()
    )]
    pub async fn workspaces_get_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::workspaces::GetInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::workspaces::get(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_ls",
        description = "List direct children of a folder.",
        output_schema = object_output_schema()
    )]
    pub async fn files_ls_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::LsInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::ls(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_stat",
        description = "Return metadata for a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_stat_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::StatInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::stat(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_mkdir",
        description = "Create a folder at a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_mkdir_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::MkdirInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::mkdir(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_touch",
        description = "Create an empty Markdown document.",
        output_schema = object_output_schema()
    )]
    pub async fn files_touch_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::TouchInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::touch(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_read",
        description = "Read a Markdown document with range limits.",
        output_schema = object_output_schema()
    )]
    pub async fn files_read_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::ReadInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::read(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_write",
        description = "Replace a Markdown document.",
        output_schema = object_output_schema()
    )]
    pub async fn files_write_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::WriteInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::write(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_patch",
        description = "Apply exact targeted replacements to one Markdown document.",
        output_schema = object_output_schema()
    )]
    pub async fn files_patch_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::PatchInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::patch(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_mv",
        description = "Move or rename a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_mv_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::MvInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::mv(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_rm",
        description = "Delete a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_rm_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files::RmInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files::rm(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_find",
        description = "Find nodes by name metadata under an optional scope path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_find_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::search::FindInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::search::find(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_grep",
        description = "Search Markdown body lines.",
        output_schema = object_output_schema()
    )]
    pub async fn files_grep_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::search::GrepInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::search::grep(&self.state, &parts, params).await
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_03_26)
            .with_server_info(
                Implementation::new("notegate", env!("CARGO_PKG_VERSION")).with_title("notegate"),
            )
            .with_instructions(MCP_SERVER_INSTRUCTIONS)
    }
}

pub async fn mcp_handler(State(state): State<AppState>, mut request: Request<Body>) -> Response {
    let (mut parts, body) = request.into_parts();
    let Some(token) = extract_bearer(&parts.headers).map(str::to_owned) else {
        return mcp_auth_response(&state, AuthError::MissingToken);
    };
    // MCP is bearer-only: try JWT → user, then the same bearer as a notegate API key.
    let caller = match verify_bearer_mcp(&state, &token).await {
        Ok(caller) => caller,
        Err(AuthError::InvalidToken | AuthError::MissingToken) => {
            match verify_api_key(&state, &token, Channel::Mcp).await {
                Ok(caller) => caller,
                Err(error) => return mcp_auth_response(&state, error),
            }
        }
        Err(error) => return mcp_auth_response(&state, error),
    };
    parts.extensions.insert(caller);
    request = Request::from_parts(parts, body);

    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(allowed_mcp_hosts(&state));
    let manager = Arc::new(NeverSessionManager::default());
    let server_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(McpServer::new(server_state.clone())),
        manager,
        config,
    );
    let response = service.handle(request).await;
    response.map(Body::new).into_response()
}

fn allowed_mcp_hosts(state: &AppState) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    push_url_host(&mut hosts, &state.config.notegate_public_url);
    push_url_host(&mut hosts, &state.config.resource_url);
    hosts.sort();
    hosts.dedup();
    hosts
}

fn push_url_host(hosts: &mut Vec<String>, raw_url: &str) {
    let Ok(url) = Url::parse(raw_url) else {
        return;
    };
    let Some(host) = url.host_str() else {
        return;
    };
    hosts.push(host.to_owned());
    if let Some(port) = url.port() {
        hosts.push(format!("{host}:{port}"));
    }
}

fn mcp_auth_response(state: &AppState, error: AuthError) -> Response {
    let status = status_for_error(&error);
    tracing::warn!(event = "mcp.auth.denied", error = %error, status = status.as_u16());
    let mut response = (status, axum::Json(auth_error_body(state, &error))).into_response();
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            WWW_AUTHENTICATE,
            shared_scoped_challenge_header(&state.config.resource_url),
        );
    }
    response
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
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    /// Building the tool router materializes every tool's input/output schema —
    /// the same path rmcp runs when answering `tools/list`. Before the fix, the
    /// `Json<Value>` file tools panicked here because rmcp cannot derive a valid
    /// MCP `outputSchema` (root `type: object`) from `serde_json::Value`. This
    /// test fails (panics) on regression and asserts every advertised
    /// `outputSchema` is an object, without needing a DB or auth token.
    #[test]
    fn every_tool_output_schema_is_a_valid_object() {
        let router = McpServer::tool_router();
        let tools = router.list_all();
        let tool_names: BTreeSet<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        let expected_tool_names = expected_tool_names();
        assert_eq!(tool_names, expected_tool_names);

        for tool in &tools {
            if let Some(schema) = &tool.output_schema {
                assert_eq!(
                    schema.get("type").and_then(Value::as_str),
                    Some("object"),
                    "tool `{}` outputSchema root must be type=object",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn every_tool_input_schema_matches_contract_fields() {
        let router = McpServer::tool_router();
        let tools: BTreeMap<_, _> = router
            .list_all()
            .into_iter()
            .map(|tool| (tool.name.to_string(), tool))
            .collect();

        assert_eq!(
            tools.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            expected_tool_names()
        );

        for (tool_name, properties, required) in [
            ("me", "", ""),
            ("workspaces_list", "limit cursor", ""),
            ("workspaces_create", "name", "name"),
            ("workspaces_get", "workspace workspace_id", ""),
            (
                "files_ls",
                "workspace workspace_id path target limit cursor",
                "",
            ),
            ("files_stat", "workspace workspace_id path target", ""),
            ("files_mkdir", "workspace workspace_id path target", ""),
            ("files_touch", "workspace workspace_id path target", ""),
            (
                "files_read",
                "workspace workspace_id path target start_line max_lines max_bytes if_none_match_sha256",
                "",
            ),
            (
                "files_write",
                "workspace workspace_id path target content_md create expected_sha256",
                "content_md",
            ),
            (
                "files_patch",
                "workspace workspace_id path target edits expected_sha256",
                "edits",
            ),
            (
                "files_mv",
                "workspace workspace_id source_path destination_path",
                "source_path destination_path",
            ),
            (
                "files_rm",
                "workspace workspace_id path target recursive",
                "",
            ),
            (
                "files_find",
                "workspace workspace_id q path target kind limit cursor",
                "q",
            ),
            (
                "files_grep",
                "workspace workspace_id q path target context limit cursor",
                "q",
            ),
        ] {
            assert_input_properties(&tools, tool_name, properties);
            assert_required_properties(&tools, tool_name, required);
        }
    }

    #[test]
    fn server_instructions_describe_all_mcp_categories() {
        assert!(MCP_SERVER_INSTRUCTIONS.contains("workspace"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("file"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("search"));
    }

    fn expected_tool_names() -> BTreeSet<&'static str> {
        BTreeSet::from([
            "me",
            "workspaces_list",
            "workspaces_create",
            "workspaces_get",
            "files_ls",
            "files_stat",
            "files_mkdir",
            "files_touch",
            "files_read",
            "files_write",
            "files_patch",
            "files_mv",
            "files_rm",
            "files_find",
            "files_grep",
        ])
    }

    fn assert_input_properties(
        tools: &BTreeMap<String, rmcp::model::Tool>,
        tool_name: &str,
        expected: &str,
    ) {
        let tool = tools.get(tool_name).expect("tool exists");
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("input schema properties object");
        for property in expected.split_whitespace() {
            assert!(
                properties.contains_key(property),
                "tool `{tool_name}` input schema missing property `{property}`"
            );
        }
    }

    fn assert_required_properties(
        tools: &BTreeMap<String, rmcp::model::Tool>,
        tool_name: &str,
        expected: &str,
    ) {
        let tool = tools.get(tool_name).expect("tool exists");
        let required: BTreeSet<_> = tool
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for property in expected.split_whitespace() {
            assert!(
                required.contains(property),
                "tool `{tool_name}` input schema should require `{property}`"
            );
        }
    }
}
