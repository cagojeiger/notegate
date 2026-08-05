import { ChevronsDownUp } from "lucide-react";
import type { DragEvent, RefObject } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { NodeSummary, Space } from "../../api/types";
import { makeRootNode } from "./nodeDisplay";
import { canMoveNodeToFolder, canMutateNode } from "./nodeWriteAccess";
import { NodeRow } from "./NodeRow";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import { projectVisibleTree, type TreeFolderSnapshot, type TreeRow } from "./treeProjection";
import type { NodeContextHandler, TreeKeyboardNavigationRegistrar } from "./types";
import { useNodeChildrenQuery } from "./useNodeQueries";
import { useTreeRestoreBatch } from "./useTreeRestoreBatch";
import { useVirtualTreeNavigation } from "./useVirtualTreeNavigation";

type TreeProps = {
  activeSpace: Space;
  openedNodeId: string | null;
  inspectedNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onInspectNode: (node: NodeSummary) => void;
  onOpenNode: (node: NodeSummary) => void;
  onNodeContextMenu: NodeContextHandler;
  onMoveNodeToFolder: (node: NodeSummary, folder: NodeSummary) => void;
  canWriteActiveSpace: boolean;
};

export function TreeSection({
  activeSpace,
  openedNodeId,
  inspectedNodeId,
  expandedFolderIds,
  open,
  onToggle,
  onCollapseTree,
  onToggleFolder,
  onInspectNode,
  onOpenNode,
  onNodeContextMenu,
  onMoveNodeToFolder,
  onTreeNavigationChange,
  canWriteActiveSpace
}: TreeProps & {
  open: boolean;
  onToggle: () => void;
  onCollapseTree: () => void;
  onTreeNavigationChange: TreeKeyboardNavigationRegistrar;
}) {
  return (
    <section id="files-section" className="flex min-h-0 min-w-0 flex-col px-3 py-1.5 font-ui">
      <SidebarSectionHeader
        label="Files"
        open={open}
        onToggle={onToggle}
        action={{ label: "Collapse all folders", icon: <ChevronsDownUp size={13} />, onClick: onCollapseTree }}
      />
      {open ? (
        <VirtualizedTree
          key={activeSpace.id}
          activeSpace={activeSpace}
          openedNodeId={openedNodeId}
          inspectedNodeId={inspectedNodeId}
          expandedFolderIds={expandedFolderIds}
          onToggleFolder={onToggleFolder}
          onInspectNode={onInspectNode}
          onOpenNode={onOpenNode}
          onNodeContextMenu={onNodeContextMenu}
          onMoveNodeToFolder={onMoveNodeToFolder}
          onTreeNavigationChange={onTreeNavigationChange}
          canWriteActiveSpace={canWriteActiveSpace}
        />
      ) : null}
    </section>
  );
}

function VirtualizedTree(props: TreeProps & { onTreeNavigationChange: TreeKeyboardNavigationRegistrar }) {
  const {
    activeSpace,
    openedNodeId,
    inspectedNodeId,
    expandedFolderIds,
    onToggleFolder,
    onInspectNode,
    onOpenNode,
    onNodeContextMenu,
    onMoveNodeToFolder,
    onTreeNavigationChange,
    canWriteActiveSpace
  } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const fetchNextPageByParent = useRef(new Map<string, () => void>());
  const [snapshots, setSnapshots] = useState<Map<string, TreeFolderSnapshot>>(() => new Map());
  const [draggedNode, setDraggedNode] = useState<NodeSummary | null>(null);
  const [dropFolderId, setDropFolderId] = useState<string | null>(null);
  const root = makeRootNode(activeSpace);
  const restoringTree = useTreeRestoreBatch(
    activeSpace.id,
    root.id,
    expandedFolderIds
  );
  const visibleTree = useMemo(
    () => projectVisibleTree(root.id, snapshots, expandedFolderIds),
    [expandedFolderIds, root.id, snapshots]
  );
  const {
    rowVirtualizer,
    virtualItems,
    handleTreeKeyDown,
    handleTreeFocusCapture,
    handleTreeBlurCapture
  } = useVirtualTreeNavigation({
    rows: visibleTree.rows,
    draggedNodeId: draggedNode?.id ?? null,
    scrollRef,
    onTreeNavigationChange
  });

  const updateSnapshot = useCallback((parentId: string, snapshot: TreeFolderSnapshot) => {
    setSnapshots((current) => {
      const previous = current.get(parentId);
      if (
        previous?.children === snapshot.children &&
        previous.isLoading === snapshot.isLoading &&
        previous.hasNextPage === snapshot.hasNextPage &&
        previous.isFetchingNextPage === snapshot.isFetchingNextPage
      ) {
        return current;
      }
      const next = new Map(current);
      next.set(parentId, snapshot);
      return next;
    });
  }, []);
  const registerFetchNextPage = useCallback((parentId: string, fetchNextPage: () => void) => {
    fetchNextPageByParent.current.set(parentId, fetchNextPage);
  }, []);

  useEffect(() => {
    const activeParentIds = new Set(visibleTree.queryParentIds);
    for (const parentId of fetchNextPageByParent.current.keys()) {
      if (!activeParentIds.has(parentId)) fetchNextPageByParent.current.delete(parentId);
    }
    setSnapshots((current) => {
      if ([...current.keys()].every((parentId) => activeParentIds.has(parentId))) return current;
      return new Map([...current].filter(([parentId]) => activeParentIds.has(parentId)));
    });
  }, [visibleTree.queryParentIds]);

  function clearDrag() {
    setDraggedNode(null);
    setDropFolderId(null);
  }

  function handleDragOver(node: NodeSummary, event: DragEvent<HTMLDivElement>) {
    if (
      !draggedNode
      || !canMoveNodeToFolder(draggedNode, node, canWriteActiveSpace)
    ) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDropFolderId(node.id);
  }

  function handleDrop(node: NodeSummary, event: DragEvent<HTMLDivElement>) {
    if (
      !draggedNode
      || !canMoveNodeToFolder(draggedNode, node, canWriteActiveSpace)
    ) return;
    event.preventDefault();
    onMoveNodeToFolder(draggedNode, node);
    clearDrag();
  }

  return (
    <>
      <div
        ref={scrollRef}
        role="tree"
        aria-label="Files"
        className="mt-0.5 min-h-0 flex-1 overflow-y-auto"
        onKeyDown={handleTreeKeyDown}
        onFocusCapture={handleTreeFocusCapture}
        onBlurCapture={handleTreeBlurCapture}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropFolderId(null);
        }}
        onContextMenu={(event) => {
          if ((event.target as HTMLElement).closest("[data-node-row]")) return;
          onNodeContextMenu(root, event);
        }}
      >
        <div className="relative w-full" style={{ height: `${rowVirtualizer.getTotalSize()}px` }}>
          {virtualItems.map((virtualRow) => {
            const row = visibleTree.rows[virtualRow.index];
            if (!row) return null;
            return (
              <div
                key={virtualRow.key}
                data-tree-index={virtualRow.index}
                className="absolute left-0 top-0 w-full pb-0.5"
                style={{ height: `${virtualRow.size}px`, transform: `translateY(${virtualRow.start}px)` }}
              >
                <VirtualTreeRow
                  row={row}
                  openedNodeId={openedNodeId}
                  inspectedNodeId={inspectedNodeId}
                  dropFolderId={dropFolderId}
                  expandedFolderIds={expandedFolderIds}
                  fetchNextPage={row.type === "load-more" ? fetchNextPageByParent.current.get(row.parentId) : undefined}
                  scrollRef={scrollRef}
                  onToggleFolder={onToggleFolder}
                  onInspectNode={onInspectNode}
                  onOpenNode={onOpenNode}
                  onNodeContextMenu={onNodeContextMenu}
                  onDragStartNode={setDraggedNode}
                  onDragOverNode={handleDragOver}
                  onDropOnNode={handleDrop}
                  onDragEndNode={clearDrag}
                  canWriteActiveSpace={canWriteActiveSpace}
                />
              </div>
            );
          })}
        </div>
      </div>
      {restoringTree ? null : visibleTree.queryParentIds.map((parentId) => (
        <FolderQueryBridge
          key={parentId}
          spaceId={activeSpace.id}
          parentId={parentId}
          onSnapshot={updateSnapshot}
          onFetchNextPage={registerFetchNextPage}
        />
      ))}
    </>
  );
}

function FolderQueryBridge({
  spaceId,
  parentId,
  onSnapshot,
  onFetchNextPage
}: {
  spaceId: string;
  parentId: string;
  onSnapshot: (parentId: string, snapshot: TreeFolderSnapshot) => void;
  onFetchNextPage: (parentId: string, fetchNextPage: () => void) => void;
}) {
  const query = useNodeChildrenQuery(spaceId, parentId, true);
  const requestNextPage = query.fetchNextPage;
  const children = useMemo(
    () => query.data?.pages.flatMap((page) => page.children) ?? [],
    [query.data?.pages]
  );
  const fetchNextPage = useCallback(() => {
    void requestNextPage();
  }, [requestNextPage]);

  useEffect(() => {
    onFetchNextPage(parentId, fetchNextPage);
  }, [fetchNextPage, onFetchNextPage, parentId]);

  useEffect(() => {
    onSnapshot(parentId, {
      children,
      isLoading: query.isLoading,
      hasNextPage: query.hasNextPage,
      isFetchingNextPage: query.isFetchingNextPage
    });
  }, [children, onSnapshot, parentId, query.hasNextPage, query.isFetchingNextPage, query.isLoading]);

  return null;
}

function VirtualTreeRow({
  row,
  openedNodeId,
  inspectedNodeId,
  dropFolderId,
  expandedFolderIds,
  fetchNextPage,
  scrollRef,
  onToggleFolder,
  onInspectNode,
  onOpenNode,
  onNodeContextMenu,
  onDragStartNode,
  onDragOverNode,
  onDropOnNode,
  onDragEndNode,
  canWriteActiveSpace
}: {
  row: TreeRow;
  openedNodeId: string | null;
  inspectedNodeId: string | null;
  dropFolderId: string | null;
  expandedFolderIds: ReadonlySet<string>;
  fetchNextPage?: () => void;
  scrollRef: RefObject<HTMLDivElement | null>;
  onToggleFolder: (nodeId: string) => void;
  onInspectNode: (node: NodeSummary) => void;
  onOpenNode: (node: NodeSummary) => void;
  onNodeContextMenu: NodeContextHandler;
  onDragStartNode: (node: NodeSummary) => void;
  onDragOverNode: (node: NodeSummary, event: DragEvent<HTMLDivElement>) => void;
  onDropOnNode: (node: NodeSummary, event: DragEvent<HTMLDivElement>) => void;
  onDragEndNode: () => void;
  canWriteActiveSpace: boolean;
}) {
  if (row.type === "loading") {
    return <div role="status" className="flex h-8 items-center py-1 text-xs text-muted" style={{ paddingLeft: `${8 + row.depth * 12}px` }}>Loading…</div>;
  }
  if (row.type === "empty") {
    return <div role="status" className="flex h-8 items-center px-2 py-2 text-xs text-muted">No items yet.</div>;
  }
  if (row.type === "load-more") {
    return (
      <LoadMoreRow
        loaded={row.loaded}
        depth={row.depth}
        isFetching={row.isFetching}
        fetchNextPage={fetchNextPage}
        scrollRef={scrollRef}
      />
    );
  }

  const node = row.node;
  return (
    <div
      role="treeitem"
      aria-level={row.depth + 1}
      aria-expanded={node.kind === "folder" ? expandedFolderIds.has(node.id) : undefined}
      aria-selected={inspectedNodeId === node.id}
    >
      <NodeRow
        node={node}
        depth={row.depth}
        inspected={inspectedNodeId === node.id}
        opened={openedNodeId === node.id}
        expanded={node.kind === "folder" && expandedFolderIds.has(node.id)}
        dropTarget={dropFolderId === node.id}
        onToggleFolder={onToggleFolder}
        onInspectNode={onInspectNode}
        onOpenNode={onOpenNode}
        onNodeContextMenu={onNodeContextMenu}
        onDragStartNode={canMutateNode(node, canWriteActiveSpace) ? onDragStartNode : undefined}
        onDragOverNode={onDragOverNode}
        onDropOnNode={onDropOnNode}
        onDragEndNode={onDragEndNode}
      />
    </div>
  );
}

function LoadMoreRow({
  loaded,
  depth,
  isFetching,
  fetchNextPage,
  scrollRef
}: {
  loaded: number;
  depth: number;
  isFetching: boolean;
  fetchNextPage?: () => void;
  scrollRef: RefObject<HTMLDivElement | null>;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element || !fetchNextPage) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !isFetching) fetchNextPage();
      },
      { root: scrollRef.current, rootMargin: "80px" }
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [fetchNextPage, isFetching, scrollRef]);

  return (
    <div ref={ref} role="status" className="flex h-8 items-center py-1 text-xs text-faint" style={{ paddingLeft: `${8 + depth * 14}px` }}>
      {isFetching ? "Loading…" : `Scroll to load more (${loaded} loaded)`}
    </div>
  );
}
