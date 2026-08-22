use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use axum::body::Body;
use axum::http::{Request, StatusCode, request::Parts};
use notegate_db::{SpaceRepo, test_support::TestDb};
use notegate_search::{
    FindMatchMode, FindRequest, GrepLineMode, GrepMatchMode, GrepRequest, SearchRunError,
    SearchRuntime,
};
use notegate_service::files::{CreateFolder, CreateText, WriteTarget, WriteText, WriteTextBody};
use serde_json::Value;
use tower::ServiceExt as _;

use super::auth::{
    InternalSearchAuth, REQUEST_SIGNATURE_HEADER, RESPONSE_SIGNATURE_HEADER, TIMESTAMP_HEADER,
};
use super::loopback_base_url;
use super::{FIND_PATH, SearchClient, SearchServerState};

#[test]
fn unspecified_search_bind_addresses_become_loopback_client_urls() {
    assert_eq!(
        loopback_base_url(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 9192))),
        "http://127.0.0.1:9192"
    );
    assert_eq!(
        loopback_base_url(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 9192))),
        "http://[::1]:9192"
    );
    assert_eq!(
        loopback_base_url(SocketAddr::from(([10, 0, 0, 8], 9192))),
        "http://10.0.0.8:9192"
    );
}

#[tokio::test]
async fn search_app_rejects_unauthorized_search_and_exposes_only_control_plane()
-> Result<(), Box<dyn std::error::Error>> {
    let pool =
        notegate_db::PgPool::connect_lazy("postgres://notegate:notegate@127.0.0.1:1/notegate")?;
    let crypto = notegate_core::security::PiiCrypto::from_root_secrets(
        "test-enc",
        &secrecy::SecretString::from("test-enc-root-secret-32-bytes-long".to_owned()),
        "test-lookup",
        &secrecy::SecretString::from("test-lookup-root-secret-32-bytes-long".to_owned()),
    )?;
    let runtime = SearchRuntime::new(
        notegate_db::FilesRepo::with_limits_and_crypto(
            pool.clone(),
            notegate_core::limits::Limits::default(),
            crypto.clone(),
        ),
        notegate_core::SearchBodyCacheConfig::default(),
        false,
    );
    let signing_key = crypto.internal_search_signing_key();
    let app = crate::routes::search_app(SearchServerState::new(
        pool,
        10,
        None,
        runtime,
        signing_key,
        None,
    ));

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("x-request-id", "search-request-123")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(
        health
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok()),
        Some("search-request-123")
    );

    for path in ["/api/v1/me", "/api/v2", "/mcp", "/mcp/v2"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }

    let unsigned_search = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(FIND_PATH)
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_eq!(unsigned_search.status(), StatusCode::NOT_FOUND);

    let original = br#"{"caller_account_id":"00000000-0000-0000-0000-000000000000"}"#;
    let tampered = br#"{"caller_account_id":"10000000-0000-0000-0000-000000000000"}"#;
    let timestamp = InternalSearchAuth::now_timestamp()?;
    let signature = InternalSearchAuth::new(signing_key)
        .sign_request(timestamp, "POST", FIND_PATH, original)?;
    let rejected_tamper = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(FIND_PATH)
                .header(TIMESTAMP_HEADER, timestamp.to_string())
                .header(REQUEST_SIGNATURE_HEADER, signature)
                .body(Body::from(tampered.as_slice()))?,
        )
        .await?;
    assert_eq!(rejected_tamper.status(), StatusCode::NOT_FOUND);

    let expired_body = serde_json::to_vec(&serde_json::json!({
        "deadline_unix_ms": 0,
        "command": {
            "caller_account_id": uuid::Uuid::nil(),
            "space_id": uuid::Uuid::nil(),
            "q": "anything",
            "path": null,
            "kind": null,
            "match_mode": "contains",
            "include": [],
            "exclude": [],
            "limit": 1,
            "cursor": null,
        }
    }))?;
    let timestamp = InternalSearchAuth::now_timestamp()?;
    let auth = InternalSearchAuth::new(signing_key);
    let signature = auth.sign_request(timestamp, "POST", FIND_PATH, &expired_body)?;
    let expired = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(FIND_PATH)
                .header(TIMESTAMP_HEADER, timestamp.to_string())
                .header(REQUEST_SIGNATURE_HEADER, signature)
                .body(Body::from(expired_body))?,
        )
        .await?;
    assert_eq!(expired.status(), StatusCode::GATEWAY_TIMEOUT);
    let response_signature = expired
        .headers()
        .get(RESPONSE_SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| std::io::Error::other("expected signed deadline response"))?
        .to_owned();
    let expired_body = axum::body::to_bytes(expired.into_body(), usize::MAX).await?;
    assert!(auth.verify_response(
        timestamp,
        StatusCode::GATEWAY_TIMEOUT.as_u16(),
        FIND_PATH,
        &expired_body,
        &response_signature,
    ));
    let expired_json: Value = serde_json::from_slice(&expired_body)?;
    assert_eq!(
        expired_json.pointer("/error/kind"),
        Some(&serde_json::json!("deadline_exceeded"))
    );
    Ok(())
}

#[tokio::test]
async fn http_search_preserves_local_mcp_find_and_grep_results()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let mut api_state = crate::rest::test_support::state(&db);
    let (caller, space_id, root_node_id) =
        crate::rest::test_support::caller_and_space(&api_state).await?;
    SpaceRepo::new(api_state.db.clone())
        .update_space(space_id, caller.account_id(), None, None, Some(true))
        .await?;
    let created = api_state
        .files
        .create_text(
            caller.account_id(),
            space_id,
            CreateText {
                parent_node_id: root_node_id,
                name: "Search Note.md".to_owned(),
            },
        )
        .await?;
    api_state
        .files
        .write_text(
            caller.account_id(),
            space_id,
            WriteText {
                target: WriteTarget::Existing {
                    node_id: created.node.node.id,
                },
                body: WriteTextBody::Plain("first line\nneedle line".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;

    let (mut parts, _) = Request::new(()).into_parts();
    parts.extensions.insert(caller.clone());
    let (local_find, local_grep) = mcp_find_and_grep(&api_state, &parts).await?;

    let signing_key = api_state.security.internal_search_signing_key();
    let store = notegate_db::FilesRepo::with_limits_and_crypto(
        api_state.db.clone(),
        api_state.config.limits,
        api_state.security.clone(),
    );
    let search_state = SearchServerState::new(
        api_state.db.clone(),
        api_state.config.db_max_connections,
        None,
        SearchRuntime::new(store, api_state.config.search_body_cache, false),
        signing_key,
        None,
    );
    let app = crate::routes::search_app(search_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    api_state.search = SearchClient::http(&base_url, signing_key)?;

    let (find, grep) = mcp_find_and_grep(&api_state, &parts).await?;
    assert_eq!(find, local_find);
    assert_eq!(grep, local_grep);
    assert_eq!(
        find.pointer("/space"),
        Some(&serde_json::json!("rest-test"))
    );
    assert_eq!(
        find.pointer("/items/0/path"),
        Some(&serde_json::json!("/Search Note.md"))
    );
    assert_eq!(
        find.pointer("/items/0/kind"),
        Some(&serde_json::json!("text"))
    );
    assert!(find.pointer("/items/0/match_lines").is_none());
    assert_eq!(find.pointer("/page/returned"), Some(&serde_json::json!(1)));
    assert_eq!(
        grep.pointer("/space"),
        Some(&serde_json::json!("rest-test"))
    );
    assert_eq!(
        grep.pointer("/items/0/path"),
        Some(&serde_json::json!("/Search Note.md"))
    );
    assert_eq!(
        grep.pointer("/items/0/match_lines"),
        Some(&serde_json::json!([2]))
    );
    assert_eq!(grep.pointer("/page/returned"), Some(&serde_json::json!(1)));

    server.abort();
    let _ = server.await;
    db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn search_runtime_routes_authority_and_queries_to_their_configured_stores()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(data_db) = TestDb::setup().await? else {
        return Ok(());
    };
    let Some(empty_db) = TestDb::setup().await? else {
        data_db.cleanup().await;
        return Ok(());
    };
    let state = crate::rest::test_support::state(&data_db);
    let (caller, space_id, _root_node_id) =
        crate::rest::test_support::caller_and_space(&state).await?;
    SpaceRepo::new(state.db.clone())
        .update_space(space_id, caller.account_id(), None, None, Some(true))
        .await?;
    let store = |pool| {
        notegate_db::FilesRepo::with_limits_and_crypto(
            pool,
            state.config.limits,
            state.security.clone(),
        )
    };
    let request = || FindRequest {
        q: "anything".to_owned(),
        path: None,
        kind: None,
        match_mode: FindMatchMode::Contains,
        include: Vec::new(),
        exclude: Vec::new(),
        limit: Some(1),
        cursor: None,
    };

    let authority_result = SearchRuntime::with_authority_and_query_stores(
        store(empty_db.pool.clone()),
        store(data_db.pool.clone()),
        state.config.search_body_cache,
        false,
    )
    .find(caller.account_id(), space_id, request())
    .await;
    assert!(matches!(
        authority_result,
        Err(SearchRunError::Search(notegate_search::SearchError::NotFound(message)))
            if message == "space not found"
    ));

    let query_result = SearchRuntime::with_authority_and_query_stores(
        store(data_db.pool.clone()),
        store(empty_db.pool.clone()),
        state.config.search_body_cache,
        false,
    )
    .find(caller.account_id(), space_id, request())
    .await;
    assert!(matches!(
        query_result,
        Err(SearchRunError::Search(notegate_search::SearchError::NotFound(message)))
            if message == "scope path not found"
    ));

    empty_db.cleanup().await;
    data_db.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn search_runtime_grep_reads_candidates_bodies_and_hydration_from_query_store()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(authority_db) = TestDb::setup().await? else {
        return Ok(());
    };
    let Some(query_db) = TestDb::setup().await? else {
        authority_db.cleanup().await;
        return Ok(());
    };
    let authority_state = crate::rest::test_support::state(&authority_db);
    let (caller, space_id, _root_node_id) =
        crate::rest::test_support::caller_and_space(&authority_state).await?;
    SpaceRepo::new(authority_state.db.clone())
        .update_space(space_id, caller.account_id(), None, None, Some(true))
        .await?;

    let query_state = crate::rest::test_support::state(&query_db);
    let (query_caller, _query_space_id, _query_root_node_id) =
        crate::rest::test_support::caller_and_space(&query_state).await?;
    sqlx::query("INSERT INTO spaces (id, owner_user_id, name) VALUES ($1, $2, $3)")
        .bind(space_id)
        .bind(query_caller.account_id())
        .bind("grep-query")
        .execute(&query_db.pool)
        .await?;
    let query_root_node_id = SpaceRepo::new(query_state.db.clone())
        .root_node_id(space_id)
        .await?
        .ok_or_else(|| std::io::Error::other("expected query-store root node"))?;
    let query_folder = query_state
        .files
        .create_folder(
            query_caller.account_id(),
            space_id,
            CreateFolder {
                parent_node_id: query_root_node_id,
                name: "Query Folder".to_owned(),
            },
        )
        .await?;
    let query_node = query_state
        .files
        .create_text(
            query_caller.account_id(),
            space_id,
            CreateText {
                parent_node_id: query_folder.node.id,
                name: "Query Body.md".to_owned(),
            },
        )
        .await?;
    query_state
        .files
        .write_text(
            query_caller.account_id(),
            space_id,
            WriteText {
                target: WriteTarget::Existing {
                    node_id: query_node.node.node.id,
                },
                body: WriteTextBody::Plain("first line\nquery-only needle".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    sqlx::query("UPDATE nodes SET write_locked = true WHERE space_id = $1 AND id = $2")
        .bind(space_id)
        .bind(query_folder.node.id)
        .execute(&query_db.pool)
        .await?;

    let store = |db: &TestDb| {
        notegate_db::FilesRepo::with_limits_and_crypto(
            db.pool.clone(),
            authority_state.config.limits,
            authority_state.security.clone(),
        )
    };
    let result = SearchRuntime::with_authority_and_query_stores(
        store(&authority_db),
        store(&query_db),
        authority_state.config.search_body_cache,
        false,
    )
    .grep(
        caller.account_id(),
        space_id,
        GrepRequest {
            q: "query-only needle".to_owned(),
            path: None,
            match_mode: GrepMatchMode::Literal,
            line_mode: GrepLineMode::First,
            include: Vec::new(),
            exclude: Vec::new(),
            limit: Some(10),
            cursor: None,
        },
    )
    .await?;

    assert_eq!(result.items.len(), 1);
    let hit = result
        .items
        .first()
        .ok_or_else(|| std::io::Error::other("expected query-store grep hit"))?;
    assert_eq!(hit.node.path, "/Query Folder/Query Body.md");
    assert_eq!(hit.match_lines, [2]);
    assert_eq!(hit.node.text.as_ref().map(|text| text.line_count), Some(2));
    assert_eq!(
        hit.node
            .write_lock_sources
            .first()
            .map(|source| (source.node_id, source.path.as_str())),
        Some((query_folder.node.id, "/Query Folder"))
    );

    query_db.cleanup().await;
    authority_db.cleanup().await;
    Ok(())
}

async fn mcp_find_and_grep(
    state: &crate::state::AppState,
    parts: &Parts,
) -> Result<(Value, Value), rmcp::ErrorData> {
    let find = crate::mcp::tools::search::find(
        state,
        parts,
        "rest-test:/".to_owned(),
        "search note".to_owned(),
        None,
        None,
        None,
        None,
        Some(10),
        None,
    )
    .await?
    .0;
    let grep = crate::mcp::tools::search::grep(
        state,
        parts,
        "rest-test:/".to_owned(),
        "needle".to_owned(),
        None,
        Some("first".to_owned()),
        None,
        None,
        Some(10),
        None,
    )
    .await?
    .0;
    Ok((find, grep))
}

#[test]
fn wire_commands_round_trip_without_losing_search_options() -> Result<(), serde_json::Error> {
    let account_id = uuid::Uuid::new_v4();
    let space_id = uuid::Uuid::new_v4();
    let find = super::contract::FindCommand::new(
        account_id,
        space_id,
        FindRequest {
            q: "note".to_owned(),
            path: Some("/docs".to_owned()),
            kind: Some(notegate_model::NodeKind::Text),
            match_mode: FindMatchMode::Glob,
            include: vec!["**/*.md".to_owned()],
            exclude: vec!["archive/**".to_owned()],
            limit: Some(7),
            cursor: Some("cursor".to_owned()),
        },
    );
    let encoded = serde_json::to_vec(&find)?;
    let encoded_json: Value = serde_json::from_slice(&encoded)?;
    assert_eq!(
        encoded_json.pointer("/match_mode"),
        Some(&serde_json::json!("glob"))
    );
    let decoded: super::contract::FindCommand = serde_json::from_slice(&encoded)?;
    let request = decoded.into_request();
    assert_eq!(request.q, "note");
    assert_eq!(request.path.as_deref(), Some("/docs"));
    assert_eq!(request.kind, Some(notegate_model::NodeKind::Text));
    assert_eq!(request.match_mode, FindMatchMode::Glob);
    assert_eq!(request.include, ["**/*.md"]);
    assert_eq!(request.exclude, ["archive/**"]);
    assert_eq!(request.limit, Some(7));
    assert_eq!(request.cursor.as_deref(), Some("cursor"));

    let grep = super::contract::GrepCommand::new(
        account_id,
        space_id,
        GrepRequest {
            q: "needle".to_owned(),
            path: None,
            match_mode: GrepMatchMode::Regex,
            line_mode: GrepLineMode::All,
            include: Vec::new(),
            exclude: Vec::new(),
            limit: None,
            cursor: None,
        },
    );
    let encoded = serde_json::to_vec(&grep)?;
    let encoded_json: Value = serde_json::from_slice(&encoded)?;
    assert_eq!(
        encoded_json.pointer("/match_mode"),
        Some(&serde_json::json!("regex"))
    );
    assert_eq!(
        encoded_json.pointer("/line_mode"),
        Some(&serde_json::json!("all"))
    );
    let decoded: super::contract::GrepCommand = serde_json::from_slice(&encoded)?;
    let request = decoded.into_request();
    assert_eq!(request.match_mode, GrepMatchMode::Regex);
    assert_eq!(request.line_mode, GrepLineMode::All);
    Ok(())
}
