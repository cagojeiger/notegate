//! Shared helpers for service integration tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_in_result
)]

use notegate_db::SpaceRepo;
#[allow(unused_imports)]
pub use notegate_db::test_support::{TestDb, deactivate_account, insert_user_account};
use notegate_service::spaces::CreateSpace;
use uuid::Uuid;

/// Create a space owned by `owner` and return `(space_id, root_id)`.
#[allow(dead_code)]
pub async fn setup_space(space_repo: &SpaceRepo, owner: Uuid, name: &str) -> (Uuid, Uuid) {
    let space = space_repo
        .create_space(
            owner,
            &CreateSpace {
                name: name.to_owned(),
            },
        )
        .await
        .expect("create space");
    let root = space_repo
        .root_node_id(space.id)
        .await
        .expect("root id query")
        .expect("root id present");
    (space.id, root)
}
