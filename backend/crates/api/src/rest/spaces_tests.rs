#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use axum::http::StatusCode;
use notegate_db::{SpaceRepo, test_support::TestDb};
use notegate_service::spaces::CreateSpace;
use serde_json::{Value, json};
use uuid::Uuid;

use super::test_support::{caller_and_space, json_request, json_response, rest_app, state};

#[tokio::test]
async fn rest_reorder_spaces_is_atomic() -> Result<(), Box<dyn std::error::Error>> {
    let Some(db) = TestDb::setup().await? else {
        return Ok(());
    };
    let state = state(&db);
    let (owner, first_id, _) = caller_and_space(&state).await?;
    sqlx::query("UPDATE users SET tier = 'system_max' WHERE id = $1")
        .bind(owner.account_id())
        .execute(&db.pool)
        .await?;
    let second = state
        .spaces
        .create(
            owner.account.kind,
            owner.account_id(),
            CreateSpace {
                name: "rest-test-second".to_owned(),
            },
        )
        .await?;

    let response = json_response(
        rest_app(state.clone(), owner.clone()),
        "POST",
        "/v1/spaces:reorder".to_owned(),
        reorder_body([(first_id, 2000), (second.space.id, 1000)]),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let repo = SpaceRepo::new(db.pool.clone());
    assert_eq!(
        repo.find_space(first_id)
            .await?
            .map(|space| space.sort_order),
        Some(2000)
    );

    let (status, error) = json_request(
        rest_app(state, owner),
        "POST",
        "/v1/spaces:reorder".to_owned(),
        reorder_body([(first_id, 3000), (Uuid::new_v4(), 4000)]),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{error}");
    assert_eq!(
        repo.find_space(first_id)
            .await?
            .map(|space| space.sort_order),
        Some(2000)
    );

    db.cleanup().await;
    Ok(())
}

fn reorder_body<const N: usize>(updates: [(Uuid, i32); N]) -> Value {
    let updates: Vec<_> = updates
        .into_iter()
        .map(|(space_id, sort_order)| {
            json!({
                "space_id": space_id,
                "sort_order": sort_order
            })
        })
        .collect();
    json!({
        "updates": updates
    })
}
