use crate::write_lock_support::{Fixture, TestResult, assert_write_locked};
use notegate_model::AccountKind;
use notegate_service::files::{
    CreateFolder, CreateText, DeleteNode, MoveNode, UpdateNode, UpdateNodeSearchPolicy,
    UpdateTextEncryption, WriteTarget, WriteText, WriteTextBody,
};

#[tokio::test]
async fn ancestor_lock_blocks_child_creation_until_unlocked() -> TestResult {
    let Some(fixture) = Fixture::setup("child-create").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "Policies").await?;
    fixture.set_lock(folder_id, true).await?;

    assert_write_locked(
        fixture
            .files
            .create_folder(
                fixture.owner,
                fixture.space_id,
                CreateFolder {
                    parent_node_id: folder_id,
                    name: "Nested".to_owned(),
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .create_text(
                fixture.owner,
                fixture.space_id,
                CreateText {
                    parent_node_id: folder_id,
                    name: "new.md".to_owned(),
                },
            )
            .await,
    );

    fixture.set_lock(folder_id, false).await?;
    fixture
        .files
        .create_text(
            fixture.owner,
            fixture.space_id,
            CreateText {
                parent_node_id: folder_id,
                name: "new.md".to_owned(),
            },
        )
        .await?;

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn locked_node_blocks_each_distinct_mutation_guard() -> TestResult {
    let Some(fixture) = Fixture::setup("node-mutations").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "Policies").await?;
    let text_id = fixture.text(folder_id, "access.md").await?;
    fixture
        .files
        .write_text(
            fixture.owner,
            fixture.space_id,
            WriteText {
                target: WriteTarget::Existing { node_id: text_id },
                body: WriteTextBody::Plain("alpha\n".to_owned()),
                expected_sha256: None,
            },
        )
        .await?;
    fixture.set_lock(folder_id, true).await?;

    assert_write_locked(
        fixture
            .files
            .write_text(
                fixture.owner,
                fixture.space_id,
                WriteText {
                    target: WriteTarget::Existing { node_id: text_id },
                    body: WriteTextBody::Plain("blocked".to_owned()),
                    expected_sha256: None,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .update_node(
                fixture.owner,
                fixture.space_id,
                UpdateNode {
                    node_id: text_id,
                    name: None,
                    sort_order: Some(2_000),
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .update_node_search_policy(
                AccountKind::User,
                fixture.owner,
                fixture.space_id,
                UpdateNodeSearchPolicy {
                    node_id: text_id,
                    enabled: false,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .update_text_encryption(
                AccountKind::User,
                fixture.owner,
                fixture.space_id,
                UpdateTextEncryption {
                    node_id: text_id,
                    enabled: true,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .delete_node(
                fixture.owner,
                fixture.space_id,
                DeleteNode {
                    node_id: text_id,
                    recursive: false,
                },
            )
            .await,
    );

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn locked_descendant_protects_subtree_structure_without_freezing_parent() -> TestResult {
    let Some(fixture) = Fixture::setup("subtree").await? else {
        return Ok(());
    };
    let folder_id = fixture.folder(fixture.root_id, "Archive").await?;
    let destination_id = fixture.folder(fixture.root_id, "Elsewhere").await?;
    let child_id = fixture.text(folder_id, "release.md").await?;
    fixture.set_lock(child_id, true).await?;

    fixture.text(folder_id, "sibling.md").await?;
    assert_write_locked(
        fixture
            .files
            .update_node(
                fixture.owner,
                fixture.space_id,
                UpdateNode {
                    node_id: folder_id,
                    name: Some("Renamed".to_owned()),
                    sort_order: None,
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .move_node(
                fixture.owner,
                fixture.space_id,
                MoveNode {
                    node_id: folder_id,
                    new_parent_node_id: destination_id,
                    new_name: None,
                    expected_parent_id: Some(fixture.root_id),
                },
            )
            .await,
    );
    assert_write_locked(
        fixture
            .files
            .delete_node(
                fixture.owner,
                fixture.space_id,
                DeleteNode {
                    node_id: folder_id,
                    recursive: true,
                },
            )
            .await,
    );

    fixture.cleanup().await;
    Ok(())
}
