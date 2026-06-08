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
        tools::me::call(&parts)
    }

    #[tool(
        name = "workspaces_list",
        description = "List workspaces accessible to the authenticated caller.",
        output_schema = object_output_schema()
    )]
    pub async fn workspaces_list_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::workspaces_list::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::workspaces_list::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "workspaces_create",
        description = "Create a workspace owned by the authenticated user caller.",
        output_schema = object_output_schema()
    )]
    pub async fn workspaces_create_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::workspaces_create::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::workspaces_create::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "workspaces_get",
        description = "Return one workspace by name.",
        output_schema = object_output_schema()
    )]
    pub async fn workspaces_get_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::workspaces_get::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::workspaces_get::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_ls",
        description = "List direct children of a folder.",
        output_schema = object_output_schema()
    )]
    pub async fn files_ls_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_ls::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_ls::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_stat",
        description = "Return metadata for a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_stat_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_stat::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_stat::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_mkdir",
        description = "Create a folder at a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_mkdir_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_mkdir::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_mkdir::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_touch",
        description = "Create an empty Markdown document.",
        output_schema = object_output_schema()
    )]
    pub async fn files_touch_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_touch::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_touch::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_read",
        description = "Read a Markdown document with range limits.",
        output_schema = object_output_schema()
    )]
    pub async fn files_read_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_read::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_read::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_write",
        description = "Replace a Markdown document.",
        output_schema = object_output_schema()
    )]
    pub async fn files_write_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_write::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_write::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_patch",
        description = "Apply exact targeted replacements to one Markdown document.",
        output_schema = object_output_schema()
    )]
    pub async fn files_patch_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_patch::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_patch::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_mv",
        description = "Move or rename a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_mv_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_mv::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_mv::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_rm",
        description = "Delete a path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_rm_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_rm::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_rm::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_find",
        description = "Find nodes by name metadata under an optional scope path.",
        output_schema = object_output_schema()
    )]
    pub async fn files_find_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_find::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_find::call(&self.state, &parts, params).await
    }

    #[tool(
        name = "files_grep",
        description = "Search Markdown body lines.",
        output_schema = object_output_schema()
    )]
    pub async fn files_grep_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::files_grep::Input>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::files_grep::call(&self.state, &parts, params).await
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
            .with_instructions("Identity tools for notegate.")
    }
}

pub async fn mcp_handler(State(state): State<AppState>, mut request: Request<Body>) -> Response {
    let (mut parts, body) = request.into_parts();
    let Some(token) = extract_bearer(&parts.headers).map(str::to_owned) else {
        return mcp_auth_response(&state, AuthError::MissingToken);
    };
    // MCP is bearer-only: try JWT → user, then the same bearer as an agent key.
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
    use super::*;

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
        assert_eq!(tools.len(), 15, "expected me + 14 file/workspace tools");
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
}
