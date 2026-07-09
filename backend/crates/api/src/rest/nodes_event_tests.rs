#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Extension;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use notegate_core::Config;
use notegate_core::security::PiiCrypto;
use notegate_db::{AccountRepo, SpaceRepo, test_support::TestDb};
use notegate_model::{Caller, CallerIdentity, Channel, ResolveAttrs};
use notegate_service::spaces::CreateSpace;
use secrecy::SecretString;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::auth::jwt::JwtAuthority;
use crate::auth::oidc::OidcProvider;
use crate::identity::{CallerResolver, IdentityError};

#[derive(Clone)]
struct UnusedResolver;

impl CallerResolver for UnusedResolver {
    fn resolve_browser(
        &self,
        _attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_browser_session_user(
        &self,
        _user_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_api(
        &self,
        _attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_mcp(
        &self,
        _attrs: ResolveAttrs,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }

    fn resolve_api_key(
        &self,
        _token: String,
        _channel: Channel,
    ) -> Pin<Box<dyn Future<Output = Result<Caller, IdentityError>> + Send + '_>> {
        Box::pin(async { Err(IdentityError::NotRegistered) })
    }
}

fn test_config() -> Arc<Config> {
    Arc::new(Config {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9191),
        database_url: "postgres://notegate:notegate@localhost/notegate".to_owned(),
        db_max_connections: 1,
        authgate_url: "https://auth.example.test".to_owned(),
        notegate_public_url: "http://localhost:9191".to_owned(),
        oauth_client_id: "notegate-web".to_owned(),
        mcp_oauth_client_id: "notegate-mcp".to_owned(),
        oauth_redirect_url: "http://localhost:9191/auth/callback".to_owned(),
        resource_url: "https://api.example.test".to_owned(),
        jwks_cache_ttl: Duration::from_secs(300),
        enc_root_key_id: "test-enc".to_owned(),
        enc_root_secret: SecretString::from("test-enc-root-secret-32-bytes-long".to_owned()),
        lookup_root_key_id: "test-lookup".to_owned(),
        lookup_root_secret: SecretString::from("test-lookup-root-secret-32-bytes-long".to_owned()),
        lookup_verify_0_key_id: None,
        lookup_verify_0_secret: None,
        browser_session_ttl: Duration::from_secs(3600),
        browser_session_max_ttl: Duration::from_secs(30 * 86_400),
        openapi_enabled: false,
        web_dist_dir: None,
        default_user_tier: notegate_core::tier::UserTier::DEFAULT,
        limits: notegate_core::limits::Limits::default(),
        secure_cookies: false,
    })
}

fn state(db: &TestDb) -> crate::state::AppState {
    let config = test_config();
    let security = PiiCrypto::from_root_secrets(
        config.enc_root_key_id.clone(),
        &config.enc_root_secret,
        config.lookup_root_key_id.clone(),
        &config.lookup_root_secret,
    )
    .expect("derive test crypto");
    notegate_service::cursor::configure_signing_key(security.session_signing_key())
        .expect("configure cursor signing key");
    let jwt = Arc::new(JwtAuthority::from_jwks(&config, aliri::Jwks::default()));
    let oidc = Arc::new(OidcProvider::new(&config, reqwest::Client::new()));
    crate::state::AppState::new(
        db.pool.clone(),
        config,
        jwt,
        oidc,
        Arc::new(UnusedResolver),
        reqwest::Client::new(),
        security,
    )
}

async fn caller_and_space(
    state: &crate::state::AppState,
) -> Result<(Caller, Uuid, Uuid), Box<dyn std::error::Error>> {
    let (account, user) = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "rest-events-owner".to_owned(),
        email: "rest-events@example.test".to_owned(),
        name: "REST Events Owner".to_owned(),
    })
    .await?;
    let space = SpaceRepo::new(state.db.clone())
        .create_space(
            account.id,
            &CreateSpace {
                name: "rest-events".to_owned(),
            },
        )
        .await?;
    let root = SpaceRepo::new(state.db.clone())
        .root_node_id(space.id)
        .await?
        .expect("root node");
    Ok((
        Caller {
            account,
            identity: CallerIdentity::User(user),
            channel: Channel::Browser,
        },
        space.id,
        root,
    ))
}

fn rest_app(state: crate::state::AppState, caller: Caller) -> Router {
    Router::new()
        .merge(super::routes())
        .merge(crate::rest::text::routes())
        .merge(crate::rest::files::routes())
        .layer(Extension(caller))
        .with_state(state)
}

async fn json_request(
    app: Router,
    method: &str,
    uri: String,
    body: Value,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))?,
        )
        .await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, value))
}

async fn get_json(
    app: Router,
    uri: String,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty())?)
        .await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok((status, serde_json::from_slice(&bytes)?))
}

async fn empty_request(
    app: Router,
    method: &str,
    uri: String,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())?,
        )
        .await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, value))
}

async fn upload_file(
    app: Router,
    space_id: Uuid,
    parent_id: Uuid,
) -> Result<(StatusCode, Value), Box<dyn std::error::Error>> {
    let boundary = "notegate-test-boundary";
    let body = format!(
        "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"parent_node_id\"\r\n\r\n\
             {parent_id}\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"name\"\r\n\r\n\
             asset.txt\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"media_type\"\r\n\r\n\
             text/plain\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"asset.txt\"\r\n\
             Content-Type: text/plain\r\n\r\n\
             asset\r\n\
             --{boundary}--\r\n"
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/spaces/{space_id}/files"))
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))?,
        )
        .await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok((status, serde_json::from_slice(&bytes)?))
}

#[tokio::test]
async fn rest_file_change_events_capture_and_list_real_mutations()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (caller, space_id, root_id) = caller_and_space(&state).await?;
    let owner = caller.account_id();

    let (status, folder) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "kind": "folder",
            "name": "docs"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{folder}");
    let folder_id: Uuid = serde_json::from_value(folder["id"].clone())?;

    let (status, text) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes"),
        json!({
            "parent_id": root_id,
            "kind": "text",
            "name": "note.md",
            "content": "hello"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    let text_id: Uuid = serde_json::from_value(text["id"].clone())?;

    let (status, file) =
        upload_file(rest_app(state.clone(), caller.clone()), space_id, root_id).await?;
    assert_eq!(status, StatusCode::CREATED, "{file}");
    let file_node_id: Uuid = serde_json::from_value(file["node"]["id"].clone())?;

    let (status, written) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/text/{text_id}"),
        json!({
            "storage_format": "plain",
            "content": "hello world"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{written}");

    let (status, no_op_write) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/text/{text_id}"),
        json!({
            "storage_format": "plain",
            "content": "hello world"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_write}");

    let (status, metadata) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/metadata"),
        json!({
            "metadata": {"source": "rest-e2e"}
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{metadata}");

    let (status, no_op_metadata) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PUT",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/metadata"),
        json!({
            "metadata": {"source": "rest-e2e"}
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_metadata}");

    let (status, updated) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PATCH",
        format!("/v1/spaces/{space_id}/nodes/{text_id}"),
        json!({
            "name": "renamed.md",
            "sort_order": 10
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{updated}");

    let (status, no_op_update) = json_request(
        rest_app(state.clone(), caller.clone()),
        "PATCH",
        format!("/v1/spaces/{space_id}/nodes/{text_id}"),
        json!({
            "name": "renamed.md",
            "sort_order": 10
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_update}");

    let (status, moved) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/move"),
        json!({
            "new_parent_id": folder_id,
            "expected_parent_id": root_id
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{moved}");

    let (status, no_op_move) = json_request(
        rest_app(state.clone(), caller.clone()),
        "POST",
        format!("/v1/spaces/{space_id}/nodes/{text_id}/move"),
        json!({
            "new_parent_id": folder_id,
            "expected_parent_id": folder_id
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{no_op_move}");

    let (status, deleted) = empty_request(
        rest_app(state.clone(), caller.clone()),
        "DELETE",
        format!("/v1/spaces/{space_id}/nodes/{text_id}"),
    )
    .await?;
    assert_eq!(status, StatusCode::NO_CONTENT, "{deleted}");

    let (status, events) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?limit=20"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{events}");
    let op_types: Vec<_> = events["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|event| event["op_type"].as_str().expect("op_type"))
        .collect();
    assert_eq!(
        op_types,
        vec![
            "item.delete",
            "item.move",
            "item.update",
            "metadata.replace",
            "text.write",
            "file.create",
            "text.create",
            "folder.create",
        ]
    );
    assert!(
        events["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["actor_account_id"] == json!(owner))
    );
    let file_event = events["events"]
        .as_array()
        .expect("events array")
        .iter()
        .find(|event| event["node_id"] == json!(file_node_id))
        .expect("file event");
    assert_eq!(file_event["metadata"]["byte_len_after"], json!(5));
    assert!(file_event["metadata"].get("line_count_after").is_none());

    let (status, first_page) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?limit=3"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{first_page}");
    assert_eq!(first_page["page"]["has_more"], json!(true));
    let cursor = first_page["page"]["next_cursor"]
        .as_str()
        .expect("next cursor");
    let (status, second_page) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?limit=3&cursor={cursor}"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{second_page}");
    assert!(
        second_page["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["op_type"] != "item.delete")
    );

    let (status, text_events) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?node_id={text_id}&limit=20"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{text_events}");
    assert_eq!(
        text_events["events"]
            .as_array()
            .expect("events array")
            .len(),
        6
    );
    let text_event_ops: Vec<_> = text_events["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|event| event["op_type"].as_str().expect("op_type"))
        .collect();
    assert_eq!(
        text_event_ops,
        vec![
            "item.delete",
            "item.move",
            "item.update",
            "metadata.replace",
            "text.write",
            "text.create",
        ]
    );
    assert!(
        text_events["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["node_id"] == json!(text_id))
    );

    let (status, invalid_cursor) = get_json(
        rest_app(state.clone(), caller.clone()),
        format!("/v1/spaces/{space_id}/file-change-events?cursor=not-a-cursor"),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid_cursor}");

    let (stranger_account, stranger_user) = AccountRepo::with_crypto_and_default_user_tier(
        state.db.clone(),
        state.security.clone(),
        state.config.default_user_tier,
    )
    .upsert_user_by_sub(&ResolveAttrs {
        sub: "rest-events-stranger".to_owned(),
        email: "stranger@example.test".to_owned(),
        name: "Stranger".to_owned(),
    })
    .await?;
    let stranger = Caller {
        account: stranger_account,
        identity: CallerIdentity::User(stranger_user),
        channel: Channel::Browser,
    };
    let (status, hidden) = get_json(
        rest_app(state.clone(), stranger),
        format!("/v1/spaces/{space_id}/file-change-events?limit=20"),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");

    db.cleanup().await;
    Ok(())
}
