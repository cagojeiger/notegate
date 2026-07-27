use notegate_db::FilesRepo;
use notegate_service::files::{
    ChildrenRequest, CopyNode, ReadText, WriteTarget, WriteText, WriteTextBody,
};
use notegate_service::search::{GrepLineMode, GrepMatchMode, GrepRequest, SearchService};

use crate::write_lock_support::{Fixture, TestResult, assert_write_locked};

#[tokio::test]
async fn inherited_lock_is_reported_without_blocking_reads() -> TestResult {
    let Some(fixture) = Fixture::setup("read-projection").await? else {
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

    let detail = fixture
        .files
        .stat(fixture.owner, fixture.space_id, text_id)
        .await?;
    assert!(!detail.node.write_locked);
    assert_eq!(detail.write_lock_sources.len(), 1);
    assert_eq!(detail.write_lock_sources[0].node_id, folder_id);
    assert_eq!(detail.write_lock_sources[0].path, "/Policies");

    let children = fixture
        .files
        .children(
            fixture.owner,
            fixture.space_id,
            folder_id,
            ChildrenRequest {
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert!(
        children
            .items
            .iter()
            .find(|item| item.node.id == text_id)
            .expect("locked child")
            .effective_write_locked
    );

    let grep = SearchService::new(FilesRepo::new(fixture.db.pool.clone()))
        .grep(
            fixture.owner,
            fixture.space_id,
            GrepRequest {
                q: "alpha".to_owned(),
                path: Some("/Policies".to_owned()),
                match_mode: GrepMatchMode::Literal,
                line_mode: GrepLineMode::First,
                include: Vec::new(),
                exclude: Vec::new(),
                limit: None,
                cursor: None,
            },
        )
        .await?;
    assert_eq!(grep.items.len(), 1);
    assert_eq!(grep.items[0].node.write_lock_sources[0].node_id, folder_id);

    let read = fixture
        .files
        .read_text(
            fixture.owner,
            fixture.space_id,
            ReadText {
                node_id: text_id,
                start_line: None,
                max_lines: None,
                max_bytes: None,
                if_none_match_sha256: None,
            },
        )
        .await?;
    assert_eq!(read.node.node.id, text_id);

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn lock_sources_follow_ancestors_and_are_not_copied() -> TestResult {
    let Some(fixture) = Fixture::setup("sources").await? else {
        return Ok(());
    };
    let outer_id = fixture.folder(fixture.root_id, "outer").await?;
    let inner_id = fixture.folder(outer_id, "inner").await?;
    let text_id = fixture.text(inner_id, "note.md").await?;
    let destination_id = fixture.folder(fixture.root_id, "destination").await?;

    for node_id in [outer_id, inner_id, text_id] {
        fixture.set_lock(node_id, true).await?;
    }
    let detail = fixture
        .files
        .stat(fixture.owner, fixture.space_id, text_id)
        .await?;
    assert_eq!(
        detail
            .write_lock_sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>(),
        ["/outer", "/outer/inner", "/outer/inner/note.md"]
    );

    fixture.set_lock(text_id, false).await?;
    fixture.set_lock(inner_id, false).await?;
    let inherited = fixture
        .files
        .stat(fixture.owner, fixture.space_id, text_id)
        .await?;
    assert!(!inherited.node.write_locked);
    assert_eq!(inherited.write_lock_sources[0].node_id, outer_id);

    fixture.set_lock(destination_id, true).await?;
    assert_write_locked(
        fixture
            .files
            .copy_node(
                fixture.owner,
                fixture.space_id,
                CopyNode {
                    node_id: outer_id,
                    new_parent_node_id: destination_id,
                    new_name: "blocked-copy".to_owned(),
                    recursive: true,
                },
            )
            .await,
    );

    let copied = fixture
        .files
        .copy_node(
            fixture.owner,
            fixture.space_id,
            CopyNode {
                node_id: outer_id,
                new_parent_node_id: fixture.root_id,
                new_name: "outer-copy".to_owned(),
                recursive: true,
            },
        )
        .await?;
    assert!(!copied.node.node.write_locked);
    let copied_descendant = fixture
        .files
        .resolve_path(fixture.owner, fixture.space_id, "/outer-copy/inner/note.md")
        .await?;
    assert!(!copied_descendant.node.write_locked);
    assert!(copied_descendant.write_lock_sources.is_empty());

    fixture.cleanup().await;
    Ok(())
}
