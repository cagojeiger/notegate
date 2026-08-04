//! rmcp 3.1.0 adapter decision:
//! - Streamable HTTP server is `rmcp::transport::streamable_http_server::StreamableHttpService`.
//! - MCP 2026-07-28 requests remain stateless. Ordinary calls prefer JSON responses, while
//!   `subscriptions/listen` stays open as SSE for tool-list change notifications.
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
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use rmcp::handler::server::tool::{Extension, ToolCallContext};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo,
    SubscriptionFilter,
};
use rmcp::service::{RequestContext, SubscriptionContext};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, Json, RoleServer, ServerHandler, tool, tool_handler, tool_router};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use notegate_model::{Caller, Channel};

use crate::auth::api_key::verify_agent_api_key;
use crate::auth::bearer::{
    AuthError, auth_error_body, extract_bearer, shared_scoped_challenge_header, status_for_error,
    verify_bearer_mcp,
};
use crate::mcp::contract::{McpAction, McpErrorData, RequiredField};
use crate::mcp::invocation;
use crate::mcp::tools;
use crate::state::AppState;

const MCP_SERVER_INSTRUCTIONS: &str = "Every tool call except `me` requires exactly one top-level `purpose`; `run_sequence` commands inherit it and must not contain `purpose`. Use `me` to inspect the caller and running server version. Use a direct tool for one operation. Prefer `run_sequence` for 2-20 commands whose inputs are known in advance and share one purpose; use separate calls when a later input depends on an earlier result such as a cursor, sha256, or discovered target. Use `read` for spaces/ls/tree/stat/read/changes, `search` for find/grep, `write` for text write/append/patch/edit, `manage` for mkdir/mv/cp/rm, and `file_transfer` for direct local file upload/download. Every paginated read uses limit, cursor, and page.next_cursor. `read op=changes` reads one Space-root mutation stream; direction defaults to older, while direction=newer replays from a stored cursor in application order. Capture checkpoint_cursor before reading a Space snapshot and save each later checkpoint_cursor only after applying every returned event; if resync_required is true, rebuild the snapshot. For any recoverable input error, use data.code and data.next_action instead of parsing the message. Targets are `<space>:/absolute/path`; space names are exact and case-sensitive, so use `read op=spaces` when unsure. Search/list before guessing paths and read/stat before modifying existing text. File bytes never pass through MCP: consume presigned URLs locally without printing or persisting them, and follow each successful file_transfer response's `next_action`. MCP cannot create, delete, or rename spaces.";
const MCP_TOOL_LIST_TTL_MS: u64 = 5 * 60 * 1_000;

/// A permissive `{"type":"object"}` output schema for the path-first file tools.
///
/// Those tools return dynamic JSON objects (`Json<Value>`); rmcp cannot
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

fn mcp_server_info() -> ServerInfo {
    ServerInfo::new(
        ServerCapabilities::builder()
            .enable_tools()
            .enable_tool_list_changed()
            .build(),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28)
    .with_server_info(
        Implementation::new("notegate", env!("CARGO_PKG_VERSION")).with_title("NoteGate"),
    )
    .with_instructions(MCP_SERVER_INSTRUCTIONS)
}

fn current_tool_list() -> ListToolsResult {
    ListToolsResult::with_all_items(McpServer::tool_router().list_all())
        .with_ttl_ms(MCP_TOOL_LIST_TTL_MS)
        .with_cache_scope(CacheScope::Public)
}

fn accepted_tool_subscription(requested: &SubscriptionFilter) -> Option<SubscriptionFilter> {
    Some(requested.supported_by(&mcp_server_info().capabilities))
}

async fn listen_for_tool_list_changes(
    context: SubscriptionContext,
    shutdown: &CancellationToken,
) -> Result<(), ErrorData> {
    if context.accepted().tools_list_changed == Some(true)
        && let Err(error) = context.sink().notify_tool_list_changed().await
    {
        tracing::debug!(
            event = "mcp.subscription.tools_list_changed_failed",
            error = %error
        );
        return Ok(());
    }

    tokio::select! {
        () = context.cancelled() => {}
        () = shutdown.cancelled() => {}
    }
    Ok(())
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

    #[tool(
        name = "me",
        description = "Show who is calling NoteGate, what this caller can generally do, and the running server version.",
        annotations(
            title = "Inspect NoteGate Identity",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn me_tool(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<Json<tools::identity::McpMeOutput>, ErrorData> {
        tools::identity::call(&parts)
    }

    #[tool(
        name = "read",
        description = "Read NoteGate spaces, nodes, text, and mutation history. Read-only. Use op=spaces/ls/tree/stat/read/changes. For changes, target a Space root (`<space>:/`); omit direction/cursor for latest events, use direction=older for history pagination, or direction=newer with a stored cursor for checkpoint replay. Every paginated op returns page.next_cursor. Space names are exact and case-sensitive.",
        annotations(title = "Read NoteGate", read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = object_output_schema()
    )]
    pub async fn read_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::unified::ReadInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::unified::read(&self.state, &parts, params).await
    }

    #[tool(
        name = "search",
        description = "Search NoteGate node names and plain text. Read-only. Use op=find or op=grep. Target space names are exact and case-sensitive; find/grep matching inside a space is case-insensitive.",
        annotations(title = "Search NoteGate", read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = false),
        output_schema = object_output_schema()
    )]
    pub async fn search_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::unified::SearchInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::unified::search(&self.state, &parts, params).await
    }

    #[tool(
        name = "write",
        description = "Create or modify plain text content. Use op=write/append/patch/edit. Does not move or delete nodes.",
        annotations(title = "Write NoteGate", read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false),
        output_schema = object_output_schema()
    )]
    pub async fn write_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::unified::WriteInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::unified::write(&self.state, &parts, params).await
    }

    #[tool(
        name = "manage",
        description = "Manage existing-space folder trees and node locations. Use op=mkdir/mv/cp/rm. Only mkdir with parents=true may target the Space root; every other target, source, or destination must name a node. MCP cannot create spaces.",
        annotations(title = "Manage NoteGate", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = object_output_schema()
    )]
    pub async fn manage_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::unified::ManageInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::unified::manage(&self.state, &parts, params).await
    }

    #[tool(
        name = "file_transfer",
        description = "Prepare direct local file transfers without sending bytes through MCP. Follow the structured next_action in every successful response. begin_upload returns one PUT or multipart geometry; for multipart, request up to 16 one-based part URLs, upload at most 4 parts concurrently using each exact content_length, collect response ETags, then complete_upload. prepare_download returns one GET. URLs expire after 5 minutes; do not print or persist them.",
        annotations(title = "Transfer NoteGate Files", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = true),
        output_schema = object_output_schema()
    )]
    pub async fn file_transfer_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::unified::FileTransferInput>,
    ) -> Result<Json<Value>, ErrorData> {
        let Parameters(input) = params;
        tools::transfers::call(&self.state, &parts, input).await
    }

    #[tool(
        name = "run_sequence",
        description = "Preferred batch tool for 2-20 commands whose inputs are known in advance. Commands inherit the one top-level purpose and use tool-discriminated flat objects without purpose or args. The server preflights every command, runs safe reads/searches concurrently, preserves dependency and mutation order, and runs mv, cp, rm, and mkdir with parents=true alone. Runtime stops on first failure; completed mutations are not rolled back.",
        annotations(title = "Run NoteGate Sequence", read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false),
        output_schema = object_output_schema()
    )]
    pub async fn run_sequence_tool(
        &self,
        Extension(parts): Extension<Parts>,
        params: Parameters<tools::unified::RunSequenceInput>,
    ) -> Result<Json<Value>, ErrorData> {
        tools::unified::run_sequence(&self.state, &parts, params).await
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        mcp_server_info()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let tool = request.name.to_string();
        let input = Value::Object(request.arguments.clone().unwrap_or_default());
        let caller = context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<Caller>())
            .cloned();
        let router = McpServer::tool_router();
        let validation_error = router
            .list_all()
            .into_iter()
            .find(|candidate| candidate.name.as_ref() == tool)
            .and_then(|definition| required_fields_error(&definition.input_schema, &input));
        let call = async move {
            if let Some(error) = validation_error {
                Err(error)
            } else {
                router
                    .call(ToolCallContext::new(self, request, context))
                    .await
            }
        };

        invocation::execute_call(&self.state, caller.as_ref(), &tool, &input, call).await
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(current_tool_list())
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        accepted_tool_subscription(requested)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        listen_for_tool_list_changes(context, &self.state.shutdown).await
    }
}

fn required_fields_error(schema: &JsonObject, input: &Value) -> Option<ErrorData> {
    let input = input.as_object()?;
    let properties = schema.get("properties").and_then(Value::as_object);
    let missing = schema
        .get("required")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .filter(|field| !input.contains_key(*field))
        .map(|field| RequiredField {
            field: field.to_owned(),
            description: properties
                .and_then(|properties| properties.get(field))
                .and_then(|property| property.get("description"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return None;
    }

    let field_names = missing
        .iter()
        .map(|field| format!("`{}`", field.field))
        .collect::<Vec<_>>()
        .join(", ");
    Some(ErrorData::invalid_params(
        format!("missing required field(s): {field_names}"),
        Some(
            McpErrorData::actionable_input(
                "required_fields_missing",
                "Add every required field described by next_action.fields and retry the same tool.",
                McpAction::AddFields { fields: missing },
            )
            .into_value(),
        ),
    ))
}

pub async fn user_mcp_handler(State(state): State<AppState>, request: Request<Body>) -> Response {
    let Some(token) = extract_bearer(request.headers()).map(str::to_owned) else {
        return user_mcp_auth_response(&state, AuthError::MissingToken);
    };
    let caller = match verify_bearer_mcp(&state, &token).await {
        Ok(caller) => caller,
        Err(error) => return user_mcp_auth_response(&state, error),
    };

    serve_mcp(state, request, caller).await
}

pub async fn agent_mcp_v2_handler(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response {
    let Some(token) = extract_bearer(request.headers()).map(str::to_owned) else {
        return agent_mcp_auth_response(&state, AuthError::MissingToken);
    };
    let caller = match verify_agent_api_key(&state, &token, Channel::Mcp).await {
        Ok(caller) => caller,
        Err(error) => return agent_mcp_auth_response(&state, error),
    };

    serve_mcp(state, request, caller).await
}

async fn serve_mcp(state: AppState, mut request: Request<Body>, caller: Caller) -> Response {
    request.extensions_mut().insert(caller);

    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
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

fn log_mcp_auth_denied(error: &AuthError, status: StatusCode) {
    let status = status.as_u16();
    match error {
        AuthError::MissingToken => {
            tracing::debug!(event = "mcp.auth.denied", error = %error, status)
        }
        AuthError::Unavailable => {
            tracing::warn!(event = "mcp.auth.denied", error = %error, status)
        }
        AuthError::Internal => tracing::error!(event = "mcp.auth.denied", error = %error, status),
        AuthError::InvalidToken | AuthError::NotRegistered | AuthError::Inactive => {
            tracing::warn!(event = "mcp.auth.denied", error = %error, status);
        }
    }
}

fn user_mcp_auth_response(state: &AppState, error: AuthError) -> Response {
    mcp_auth_response(
        state,
        error,
        shared_scoped_challenge_header(&state.config.resource_url),
    )
}

fn agent_mcp_auth_response(state: &AppState, error: AuthError) -> Response {
    mcp_auth_response(
        state,
        error,
        HeaderValue::from_static("Bearer realm=\"notegate-agent-mcp-v2\""),
    )
}

fn mcp_auth_response(state: &AppState, error: AuthError, challenge: HeaderValue) -> Response {
    let status = status_for_error(&error);
    log_mcp_auth_denied(&error, status);
    let mut response = (status, axum::Json(auth_error_body(state, &error))).into_response();
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(WWW_AUTHENTICATE, challenge);
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
    use notegate_db::SpaceRepo;
    use rmcp::model::{CallToolResult, ClientInfo, ContentBlock, ServerNotification};
    use rmcp::service::SubscriptionEnd;
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
    use rmcp::{ClientLifecycleMode, ClientServiceExt};
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn required_fields_error_uses_the_published_schema_and_action_contract() {
        let read = McpServer::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "read")
            .expect("read tool exists");

        let error = required_fields_error(&read.input_schema, &json!({"op": "spaces"}))
            .expect("purpose is required");
        let data = error.data.expect("required field error carries data");

        assert_eq!(data["code"], "required_fields_missing");
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["next_action"]["kind"], "add_fields");
        assert_eq!(data["next_action"]["fields"][0]["field"], "purpose");
        assert!(
            data["next_action"]["fields"][0]["description"]
                .as_str()
                .is_some_and(|description| description.contains("MCP invocation"))
        );
        assert!(
            required_fields_error(
                &read.input_schema,
                &json!({
                    "purpose": "list available spaces",
                    "op": "spaces"
                })
            )
            .is_none()
        );
    }

    #[derive(Clone)]
    struct ToolRefreshTestServer {
        shutdown: CancellationToken,
    }

    impl ServerHandler for ToolRefreshTestServer {
        fn get_info(&self) -> ServerInfo {
            mcp_server_info()
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            Ok(current_tool_list())
        }

        fn accepted_subscription_filter(
            &self,
            requested: &SubscriptionFilter,
        ) -> Option<SubscriptionFilter> {
            accepted_tool_subscription(requested)
        }

        async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
            listen_for_tool_list_changes(context, &self.shutdown).await
        }
    }

    async fn spawn_tool_refresh_test_server()
    -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
        let shutdown = CancellationToken::new();
        let handler_shutdown = shutdown.clone();
        let service: StreamableHttpService<ToolRefreshTestServer, NeverSessionManager> =
            StreamableHttpService::new(
                move || {
                    Ok(ToolRefreshTestServer {
                        shutdown: handler_shutdown.clone(),
                    })
                },
                Arc::new(NeverSessionManager::default()),
                StreamableHttpServerConfig::default()
                    .with_legacy_session_mode(false)
                    .with_json_response(true)
                    .with_sse_keep_alive(Some(Duration::from_millis(20))),
            );
        let router = axum::Router::new().nest_service("/mcp", service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test MCP server");
        let address = listener.local_addr().expect("test server address");
        let server_task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(shutdown.cancelled_owned())
                    .await
                    .expect("serve test MCP server");
            }
        });
        (format!("http://{address}/mcp"), shutdown, server_task)
    }

    async fn spawn_invocation_test_server(
        state: AppState,
        caller: Caller,
    ) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
        let shutdown = CancellationToken::new();
        let server_state = state.clone();
        let service: StreamableHttpService<McpServer, NeverSessionManager> =
            StreamableHttpService::new(
                move || Ok(McpServer::new(server_state.clone())),
                Arc::new(NeverSessionManager::default()),
                StreamableHttpServerConfig::default()
                    .with_legacy_session_mode(false)
                    .with_json_response(true),
            );
        let router = axum::Router::new()
            .nest_service("/mcp", service)
            .layer(axum::Extension(caller));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind invocation test MCP server");
        let address = listener
            .local_addr()
            .expect("invocation test server address");
        let server_task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(shutdown.cancelled_owned())
                    .await
                    .expect("serve invocation test MCP server");
            }
        });
        (format!("http://{address}/mcp"), shutdown, server_task)
    }

    async fn stop_tool_refresh_test_server(
        client: rmcp::service::RunningService<rmcp::RoleClient, ClientInfo>,
        shutdown: CancellationToken,
        server_task: tokio::task::JoinHandle<()>,
    ) {
        client.cancel().await.expect("close MCP client");
        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("test server shuts down")
            .expect("test server task joins");
    }

    #[test]
    fn server_advertises_modern_tool_list_refresh() {
        let info = mcp_server_info();
        let tools = info
            .capabilities
            .tools
            .as_ref()
            .expect("tools capability is advertised");

        assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            info.server_info.version,
            include_str!("../../../../../VERSION").trim(),
            "the compiled server version must match the release source of truth"
        );
        assert_eq!(tools.list_changed, Some(true));

        let requested = SubscriptionFilter::builder().tools_list_changed().build();
        let accepted = requested.supported_by(&info.capabilities);
        assert_eq!(accepted.tools_list_changed, Some(true));
        assert_eq!(accepted.prompts_list_changed, None);
    }

    #[test]
    fn tool_list_has_bounded_public_cache_policy() {
        let result = current_tool_list();

        assert_eq!(result.ttl_ms, Some(MCP_TOOL_LIST_TTL_MS));
        assert_eq!(result.cache_scope, Some(CacheScope::Public));
        assert_eq!(result.tools.len(), expected_tool_names().len());
    }

    #[tokio::test]
    async fn modern_http_subscription_refreshes_tools_and_stays_open() {
        let (url, shutdown, server_task) = spawn_tool_refresh_test_server().await;

        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url),
        );
        let client = ClientInfo::default()
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .expect("connect modern MCP client");

        let listed = client.list_tools(None).await.expect("list tools");
        assert_eq!(listed.ttl_ms, Some(MCP_TOOL_LIST_TTL_MS));
        assert_eq!(listed.cache_scope, Some(CacheScope::Public));

        let mut subscription = client
            .listen(SubscriptionFilter::builder().tools_list_changed().build())
            .await
            .expect("open tool-list subscription");
        assert!(matches!(
            subscription
                .next()
                .await
                .expect("read subscription")
                .expect("receive initial refresh"),
            ServerNotification::ToolListChangedNotification(_)
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), subscription.next())
                .await
                .is_err(),
            "subscription must remain open after the initial refresh"
        );

        subscription.cancel().await.expect("cancel subscription");
        stop_tool_refresh_test_server(client, shutdown, server_task).await;
    }

    #[tokio::test]
    async fn server_shutdown_gracefully_closes_active_subscription() {
        let (url, shutdown, server_task) = spawn_tool_refresh_test_server().await;
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url),
        );
        let client = ClientInfo::default()
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .expect("connect modern MCP client");
        let mut subscription = client
            .listen(SubscriptionFilter::builder().tools_list_changed().build())
            .await
            .expect("open tool-list subscription");
        assert!(
            subscription
                .next()
                .await
                .expect("read initial refresh")
                .is_some()
        );

        shutdown.cancel();

        assert!(
            tokio::time::timeout(Duration::from_secs(5), subscription.next())
                .await
                .expect("subscription closes after server shutdown")
                .expect("read subscription end")
                .is_none()
        );
        assert!(matches!(
            subscription.end(),
            Some(SubscriptionEnd::Graceful(_))
        ));
        let _ = client.cancel().await;
        tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("test server shuts down")
            .expect("test server task joins");
    }

    #[tokio::test]
    async fn legacy_initialize_clients_can_still_list_tools() {
        let (url, shutdown, server_task) = spawn_tool_refresh_test_server().await;
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url),
        );
        let client = ClientInfo::default()
            .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
            .await
            .expect("connect legacy MCP client");

        let listed = client.list_tools(None).await.expect("legacy tools/list");
        assert_eq!(listed.tools.len(), expected_tool_names().len());

        stop_tool_refresh_test_server(client, shutdown, server_task).await;
    }

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
            (
                "read",
                "purpose op target name depth limit cursor direction start_line max_lines max_bytes if_none_match_sha256",
                "purpose op",
            ),
            (
                "search",
                "purpose op target q kind match lines include exclude limit cursor",
                "purpose op target q",
            ),
            (
                "write",
                "purpose op target content edits create ensure_newline expected_sha256",
                "purpose op target",
            ),
            (
                "manage",
                "purpose op target source destination parents recursive",
                "purpose op",
            ),
            (
                "file_transfer",
                "purpose op target byte_len media_type original_filename encryption_mode encryption_metadata upload_id part_numbers completed_parts",
                "purpose op",
            ),
            ("run_sequence", "purpose commands", "purpose commands"),
        ] {
            assert_input_properties(&tools, tool_name, properties);
            assert_required_properties(&tools, tool_name, required);
        }

        let common_purpose_description = "Reason for this MCP invocation. Required once at the top level; maximum 200 characters.";
        for tool_name in ["read", "search", "write", "manage", "file_transfer"] {
            assert_eq!(
                tools[tool_name].input_schema["properties"]["purpose"]["description"],
                common_purpose_description,
                "tool `{tool_name}` must expose the shared invocation-purpose contract"
            );
        }
        assert_eq!(
            tools["run_sequence"].input_schema["properties"]["purpose"]["description"],
            "Reason for this MCP invocation. Commands inherit it; maximum 200 characters."
        );

        let me_properties = tools
            .get("me")
            .and_then(|tool| tool.input_schema.get("properties"))
            .and_then(Value::as_object)
            .expect("me input properties exist");
        assert!(me_properties.is_empty(), "me must remain input-free");

        let me_output_properties = tools
            .get("me")
            .and_then(|tool| tool.output_schema.as_ref())
            .and_then(|schema| schema.get("properties"))
            .and_then(Value::as_object)
            .expect("me output properties exist");
        for property in ["account", "user", "agent", "capabilities", "server_version"] {
            assert!(
                me_output_properties.contains_key(property),
                "me output schema should contain `{property}`"
            );
        }

        let write_edits = tools
            .get("write")
            .and_then(|tool| tool.input_schema.get("properties"))
            .and_then(|properties| properties.get("edits"))
            .expect("write edits schema exists");
        assert_write_edit_schema(write_edits);

        let sequence_commands = tools
            .get("run_sequence")
            .and_then(|tool| tool.input_schema.get("properties"))
            .and_then(|properties| properties.get("commands"))
            .expect("sequence commands schema exists");
        assert_eq!(sequence_commands["minItems"], 1);
        assert_eq!(sequence_commands["maxItems"], 20);
        let sequence_command = sequence_commands
            .get("items")
            .expect("sequence command item schema exists");
        assert!(sequence_command.get("$ref").is_none());
        let sequence_variants = sequence_command
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("sequence command exposes tool-discriminated variants");
        assert_eq!(sequence_variants.len(), 4);
        let sequence_variants = sequence_variants
            .iter()
            .map(|variant| {
                let properties = variant
                    .get("properties")
                    .and_then(Value::as_object)
                    .expect("sequence variant properties exist");
                assert!(!properties.contains_key("purpose"));
                assert!(!properties.contains_key("args"));
                assert_eq!(variant["additionalProperties"], false);
                let tool = properties["tool"]["const"]
                    .as_str()
                    .expect("sequence variant fixes one tool");
                (tool, variant)
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            sequence_variants.keys().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from(["manage", "read", "search", "write"])
        );
        for (tool, operations) in [
            (
                "read",
                json!(["spaces", "ls", "tree", "stat", "read", "changes"]),
            ),
            ("search", json!(["find", "grep"])),
            ("write", json!(["write", "append", "patch", "edit"])),
            ("manage", json!(["mkdir", "mv", "cp", "rm"])),
        ] {
            assert_eq!(
                sequence_variants[tool]["properties"]["op"]["enum"], operations,
                "sequence variant must expose only operations for {tool}"
            );
        }
        for tool in ["read", "search", "write", "manage"] {
            let direct_fields = tools[tool].input_schema["properties"]
                .as_object()
                .expect("direct tool properties exist")
                .keys()
                .filter(|field| field.as_str() != "purpose")
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let sequence_fields = sequence_variants[tool]["properties"]
                .as_object()
                .expect("sequence variant properties exist")
                .keys()
                .filter(|field| field.as_str() != "tool")
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            assert_eq!(
                sequence_fields, direct_fields,
                "sequence variant must reuse the direct {tool} field vocabulary"
            );
        }
        assert!(sequence_variants["read"]["properties"].get("q").is_none());
        assert!(
            sequence_variants["search"]["properties"]
                .get("content")
                .is_none()
        );
        assert!(
            sequence_variants["write"]["properties"]
                .get("source")
                .is_none()
        );
        assert!(
            sequence_variants["manage"]["properties"]
                .get("cursor")
                .is_none()
        );
        assert_eq!(
            sequence_variants["write"]["properties"]["create"]["type"],
            "boolean"
        );
        assert_eq!(
            sequence_variants["manage"]["properties"]["recursive"]["type"],
            "boolean"
        );
        let sequence_edits = sequence_variants["write"]
            .pointer("/properties/edits")
            .expect("sequence write edits schema exists");
        assert_write_edit_schema(sequence_edits);

        let sequence = tools.get("run_sequence").expect("run_sequence tool exists");
        let description = sequence
            .description
            .as_deref()
            .expect("run_sequence description exists");
        assert!(description.contains("Preferred batch tool"));
        assert!(description.contains("inherit the one top-level purpose"));
        assert!(description.contains("tool-discriminated flat objects"));
        assert!(description.contains("without purpose or args"));
        assert!(description.contains("preflights every command"));
        assert!(description.contains("runs mv, cp, rm"));

        let read = tools.get("read").expect("read tool exists");
        let description = read
            .description
            .as_deref()
            .expect("read description exists");
        assert!(description.contains("op=spaces/ls/tree/stat/read/changes"));
        assert!(description.contains("<space>:/"));
        assert!(description.contains("direction=newer"));
        let properties = read
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("read properties exist");
        assert!(properties.contains_key("direction"));
        assert_eq!(
            properties["op"].get("description").and_then(Value::as_str),
            Some("Operation: spaces/ls/tree/stat/read/changes.")
        );
        assert!(
            properties["target"]
                .get("description")
                .and_then(Value::as_str)
                .is_some_and(|description| description.contains("Space root `<space>:/`"))
        );
        assert!(!properties.contains_key("before"));
        assert!(!properties.contains_key("after"));
        assert!(!properties.contains_key("mode"));
        assert!(!properties.contains_key("after_event_id"));
    }

    #[test]
    fn me_tool_annotations_match_its_read_only_behavior() {
        let me = McpServer::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "me")
            .expect("me tool exists");
        let annotations = me.annotations.as_ref().expect("me annotations exist");

        assert_eq!(
            annotations.title.as_deref(),
            Some("Inspect NoteGate Identity")
        );
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
    }

    #[test]
    fn server_instructions_describe_all_mcp_categories() {
        assert!(MCP_SERVER_INSTRUCTIONS.contains("space"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("except `me`"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("exactly one top-level `purpose`"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("2-20 commands"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("later input depends"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("read"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("page.next_cursor"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("checkpoint_cursor"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("data.code"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("data.next_action"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("search"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("write"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("manage"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("file_transfer"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("next_action"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("run_sequence"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("cannot create"));

        let file_transfer = McpServer::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "file_transfer")
            .expect("file_transfer tool exists");
        assert!(
            file_transfer
                .description
                .as_deref()
                .is_some_and(|description| description.contains("next_action"))
        );

        let read = McpServer::tool_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "read")
            .expect("read tool exists");
        let description = read
            .description
            .as_deref()
            .expect("read description exists");
        assert!(description.contains("page.next_cursor"));
        assert!(description.contains("older"));
        assert!(description.contains("newer"));
    }

    fn expected_tool_names() -> BTreeSet<&'static str> {
        BTreeSet::from([
            "me",
            "read",
            "search",
            "write",
            "manage",
            "file_transfer",
            "run_sequence",
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

    #[tokio::test]
    async fn sequence_preflight_prevents_partial_writes() -> Result<(), Box<dyn std::error::Error>>
    {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller);
        let input = serde_json::from_value(serde_json::json!({
            "purpose": "prove invalid later changes cannot leave partial writes",
            "commands": [
                {
                    "tool": "write",
                    "op": "write",
                    "target": "rest-test:/should-not-exist.md",
                    "content": "must not be committed",
                    "create": true
                },
                {
                    "tool": "write",
                    "op": "patch",
                    "target": "rest-test:/does-not-matter.md",
                    "edits": [{
                        "old_text": "before",
                        "new_text": "after",
                        "mode": "latest"
                    }]
                },
                {
                    "tool": "read",
                    "op": "changes",
                    "target": "rest-test:/",
                    "direction": "newer"
                }
            ]
        }))?;

        let error = match tools::unified::run_sequence(&state, &parts, Parameters(input)).await {
            Ok(_) => panic!("missing changes cursor must fail preflight"),
            Err(error) => error,
        };
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data["executed"].as_bool()),
            Some(false)
        );
        let errors = error
            .data
            .as_ref()
            .and_then(|data| data["errors"].as_array())
            .expect("preflight errors");
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0]["index"], 1);
        assert_eq!(errors[1]["index"], 2);
        let stat =
            tools::files::stat(&state, &parts, "rest-test:/should-not-exist.md".to_owned()).await;
        assert!(stat.is_err(), "the earlier write must not have run");

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn sequence_preflight_rejects_invalid_static_write_content_without_partial_writes()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller);
        let input = serde_json::from_value(serde_json::json!({
            "purpose": "reject invalid structured text before writing",
            "commands": [
                {
                    "tool": "write",
                    "op": "write",
                    "target": "rest-test:/should-not-exist.md",
                    "content": "must not be committed",
                    "create": true
                },
                {
                    "tool": "write",
                    "op": "write",
                    "target": "rest-test:/config.json",
                    "content": "{\"ok\":}",
                    "create": true
                }
            ]
        }))?;

        let error = match tools::unified::run_sequence(&state, &parts, Parameters(input)).await {
            Ok(_) => panic!("invalid JSON must fail sequence preflight"),
            Err(error) => error,
        };
        let data = error.data.as_ref().expect("structured preflight data");
        assert_eq!(data["code"], "sequence_preflight_failed");
        assert_eq!(data["executed"], false);
        assert_eq!(data["errors"].as_array().map(Vec::len), Some(1));
        assert_eq!(data["errors"][0]["index"], 1);
        assert!(
            data["errors"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("invalid json syntax in config.json"))
        );
        let stat =
            tools::files::stat(&state, &parts, "rest-test:/should-not-exist.md".to_owned()).await;
        assert!(stat.is_err(), "the earlier write must not have run");

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn sequence_preflight_rejects_manage_root_targets_without_partial_writes()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller);
        let invalid_commands = [
            serde_json::json!({"tool": "manage", "op": "mkdir", "target": "rest-test:/"}),
            serde_json::json!({"tool": "manage", "op": "rm", "target": "rest-test:/", "recursive": true}),
            serde_json::json!({"tool": "manage", "op": "mv", "source": "rest-test:/", "destination": "rest-test:/moved"}),
            serde_json::json!({"tool": "manage", "op": "mv", "source": "rest-test:/source", "destination": "rest-test:/"}),
            serde_json::json!({"tool": "manage", "op": "cp", "source": "rest-test:/", "destination": "rest-test:/copied", "recursive": true}),
            serde_json::json!({"tool": "manage", "op": "cp", "source": "rest-test:/source", "destination": "rest-test:/", "recursive": true}),
        ];

        for (index, invalid_command) in invalid_commands.into_iter().enumerate() {
            let created_target = format!("rest-test:/root-preflight-{index}.md");
            let input = serde_json::from_value(serde_json::json!({
                "purpose": "reject root manage commands before writing",
                "commands": [
                    {
                        "tool": "write",
                        "op": "write",
                        "target": created_target,
                        "content": "must not be committed",
                        "create": true
                    },
                    invalid_command
                ]
            }))?;

            let error = match tools::unified::run_sequence(&state, &parts, Parameters(input)).await
            {
                Ok(_) => panic!("root manage target must fail sequence preflight"),
                Err(error) => error,
            };
            let data = error.data.as_ref().expect("structured preflight data");
            assert_eq!(data["code"], "sequence_preflight_failed");
            assert_eq!(data["executed"], false);
            assert_eq!(data["errors"].as_array().map(Vec::len), Some(1));
            assert_eq!(data["errors"][0]["index"], 1);
            let stat = tools::files::stat(&state, &parts, created_target).await;
            assert!(stat.is_err(), "the earlier write must not have run");
        }

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn sequence_parent_stat_observes_a_created_child()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller);
        tools::files::mkdir(&state, &parts, "rest-test:/parent".to_owned(), false).await?;
        let input = serde_json::from_value(serde_json::json!({
            "purpose": "create a child before reading parent metadata",
            "commands": [
                {
                    "tool": "write",
                    "op": "write",
                    "target": "rest-test:/parent/child.md",
                    "content": "child",
                    "create": true
                },
                {
                    "tool": "read",
                    "op": "stat",
                    "target": "rest-test:/parent"
                }
            ]
        }))?;

        let Json(result) = tools::unified::run_sequence(&state, &parts, Parameters(input)).await?;
        assert_eq!(result["ok"], true);
        assert_eq!(result["completed"], 2);
        assert_eq!(result["results"][1]["result"]["node"]["has_children"], true);

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn changes_invocation_records_its_space_name() -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        SpaceRepo::new(state.db.clone())
            .update_space(space_id, caller.account_id(), None, None, Some(true))
            .await?;
        let mut parts = axum::http::Request::new(()).into_parts().0;
        parts.extensions.insert(caller.clone());
        let input = serde_json::json!({
            "purpose": "Review recent changes",
            "op": "changes",
            "target": "rest-test:/",
            "limit": 1
        });

        let server = McpServer::new(state.clone());
        let typed_input = serde_json::from_value(input.clone())?;
        invocation::execute_call(&state, Some(&caller), "read", &input, async {
            server
                .read_tool(Extension(parts), Parameters(typed_input))
                .await
                .map(|value| CallToolResult::structured(value.0).into())
        })
        .await?;

        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                serde_json::Value,
                serde_json::Value,
            ),
        >(
            "SELECT tool, op, space_name, input, response FROM mcp_invocations \
             WHERE actor_account_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(caller.account_id())
        .fetch_one(&state.db)
        .await?;
        assert_eq!(row.0, "read");
        assert_eq!(row.1, "changes");
        assert_eq!(row.2.as_deref(), Some("rest-test"));
        assert_eq!(row.3, input);
        assert_eq!(row.4["result"]["space"], "rest-test");

        let invalid_input = serde_json::json!({
            "purpose": "Review recent changes",
            "op": "changes",
            "target": "rest-test:/not-root"
        });
        let mut invalid_parts = axum::http::Request::new(()).into_parts().0;
        invalid_parts.extensions.insert(caller.clone());
        let typed_invalid_input = serde_json::from_value(invalid_input.clone())?;
        invocation::execute_call(&state, Some(&caller), "read", &invalid_input, async {
            server
                .read_tool(Extension(invalid_parts), Parameters(typed_invalid_input))
                .await
                .map(|value| CallToolResult::structured(value.0).into())
        })
        .await
        .expect_err("changes rejects a non-root target");

        let failed = sqlx::query_as::<_, (Option<String>, String, Option<String>)>(
            "SELECT space_name, outcome, error_code FROM mcp_invocations \
             WHERE actor_account_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(caller.account_id())
        .fetch_one(&state.db)
        .await?;
        assert_eq!(failed.0.as_deref(), Some("rest-test"));
        assert_eq!(failed.1, "error");
        assert_eq!(failed.2.as_deref(), Some("changes_scope_invalid"));

        let missing_purpose = serde_json::json!({"op": "spaces"});
        let malformed =
            invocation::execute_call(&state, Some(&caller), "read", &missing_purpose, async {
                Ok(CallToolResult::error(vec![ContentBlock::text(
                    "failed to deserialize parameters: missing field `purpose`",
                )])
                .into())
            })
            .await?;
        assert!(matches!(
            malformed,
            CallToolResponse::Complete(ref result) if result.is_error == Some(true)
        ));

        let malformed_row = sqlx::query_as::<
            _,
            (
                Option<String>,
                serde_json::Value,
                serde_json::Value,
                String,
                String,
            ),
        >(
            "SELECT purpose, input, response, outcome, error_code FROM mcp_invocations \
             WHERE actor_account_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(caller.account_id())
        .fetch_one(&state.db)
        .await?;
        assert_eq!(malformed_row.0, None);
        assert_eq!(malformed_row.1, missing_purpose);
        assert_eq!(malformed_row.2["kind"], "complete");
        assert_eq!(malformed_row.2["is_error"], true);
        assert_eq!(malformed_row.3, "error");
        assert_eq!(malformed_row.4, "invalid_params");
        assert!(!malformed_row.2.to_string().contains("missing field"));

        db.cleanup().await;
        Ok(())
    }

    #[tokio::test]
    async fn tools_call_boundary_records_schema_and_unknown_tool_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(db) = notegate_db::test_support::TestDb::setup().await? else {
            return Ok(());
        };
        let state = crate::rest::test_support::state(&db);
        let (caller, _space_id, _root_id) =
            crate::rest::test_support::caller_and_space(&state).await?;
        let (url, shutdown, server_task) =
            spawn_invocation_test_server(state.clone(), caller.clone()).await;
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url),
        );
        let client = ClientInfo::default()
            .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
            .await?;

        let missing_purpose = serde_json::json!({"op": "spaces"});
        let result = client
            .call_tool(
                CallToolRequestParams::new("read")
                    .with_arguments(serde_json::from_value(missing_purpose.clone())?),
            )
            .await;
        assert!(result.is_err());

        let unknown_input = serde_json::json!({
            "purpose": "test unknown tool",
            "value": "SECRET_UNKNOWN_ARGUMENT"
        });
        let unknown_error = client
            .call_tool(
                CallToolRequestParams::new("not_a_tool")
                    .with_arguments(serde_json::from_value(unknown_input.clone())?),
            )
            .await;
        assert!(unknown_error.is_err());

        let rows = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                serde_json::Value,
                serde_json::Value,
                String,
                String,
            ),
        >(
            "SELECT tool, purpose, input, response, outcome, error_code FROM mcp_invocations \
             WHERE actor_account_id = $1 ORDER BY id",
        )
        .bind(caller.account_id())
        .fetch_all(&state.db)
        .await?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "read");
        assert_eq!(rows[0].1, None);
        assert_eq!(rows[0].2, missing_purpose);
        assert_eq!(rows[0].3["kind"], "error");
        assert_eq!(rows[0].4, "error");
        assert_eq!(rows[0].5, "required_fields_missing");
        assert_eq!(rows[1].0, "unknown");
        assert_eq!(rows[1].1.as_deref(), Some("test unknown tool"));
        assert_eq!(rows[1].2["purpose"], "test unknown tool");
        assert_eq!(
            rows[1].2["_redacted_payload"]["category"],
            "unrecognized_tool_input"
        );
        assert!(!rows[1].2.to_string().contains("SECRET_UNKNOWN_ARGUMENT"));
        assert_eq!(rows[1].3["kind"], "error");
        assert_eq!(rows[1].4, "error");

        stop_tool_refresh_test_server(client, shutdown, server_task).await;
        db.cleanup().await;
        Ok(())
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

    fn assert_write_edit_schema(schema: &Value) {
        let items = schema.get("items").expect("edits array items exist");
        assert_ne!(items, &Value::Bool(true), "edits must not be unconstrained");
        let variants = items
            .get("anyOf")
            .and_then(Value::as_array)
            .expect("edits items expose patch and line-edit variants");
        assert_eq!(variants.len(), 2);

        let patch = variants
            .iter()
            .find(|variant| variant.pointer("/properties/old_text").is_some())
            .expect("patch edit schema exists");
        assert_eq!(patch.get("additionalProperties"), Some(&Value::Bool(false)));
        assert_schema_requires(patch, &["old_text", "new_text"]);

        let line = variants
            .iter()
            .find(|variant| variant.pointer("/properties/op").is_some())
            .expect("line edit schema exists");
        assert_eq!(line.get("additionalProperties"), Some(&Value::Bool(false)));
        assert_schema_requires(line, &["op"]);
    }

    fn assert_schema_requires(schema: &Value, expected: &[&str]) {
        let required: BTreeSet<_> = schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for field in expected {
            assert!(required.contains(field), "schema should require `{field}`");
        }
    }
}
