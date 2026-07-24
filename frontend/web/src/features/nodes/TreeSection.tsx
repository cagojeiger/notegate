import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import { ChevronsDownUp, Folder } from "lucide-react";
import type { DragEvent, KeyboardEvent as ReactKeyboardEvent, RefObject } from "react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { RestNode } from "../../entities/node/model";
import type { Space } from "../../entities/space/model";
import { makeRootNode, nodeMetaSuffix } from "./nodeDisplay";
import { NodeRow } from "./NodeRow";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import { findAdjacentNodeRowIndex, projectVisibleTree, type TreeFolderSnapshot, type TreeRow } from "./treeProjection";
import type { NodeContextHandler, TreeKeyboardNavigationRegistrar } from "./types";
import { useNodeChildrenQuery } from "./useNodeQueries";

const TREE_ROW_SIZE = 36;
const TREE_OVERSCAN = 8;

type TreeProps = {
  activeSpace: Space;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onNodeContextMenu: NodeContextHandler;
  onMoveNodeToFolder: (node: RestNode, folder: RestNode) => void;
  canWriteActiveSpace: boolean;
};

export function TreeSection({
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  open,
  onToggle,
  onCollapseTree,
  onToggleFolder,
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
    <section className="flex min-h-0 min-w-0 flex-col px-3 py-2">
      <SidebarSectionHeader
        icon={<Folder size={13} />}
        label="Files"
        open={open}
        onToggle={onToggle}
        action={{ label: "Collapse all folders", icon: <ChevronsDownUp size={13} />, onClick: onCollapseTree }}
      />
      {open ? (
        <VirtualizedTree
          key={activeSpace.id}
          activeSpace={activeSpace}
          activeNodeId={activeNodeId}
          expandedFolderIds={expandedFolderIds}
          onToggleFolder={onToggleFolder}
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
    activeNodeId,
    expandedFolderIds,
    onToggleFolder,
    onOpenNode,
    onNodeContextMenu,
    onMoveNodeToFolder,
    onTreeNavigationChange,
    canWriteActiveSpace
  } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const fetchNextPageByParent = useRef(new Map<string, () => void>());
  const [snapshots, setSnapshots] = useState<Map<string, TreeFolderSnapshot>>(() => new Map());
  const [draggedNode, setDraggedNode] = useState<RestNode | null>(null);
  const [dropFolderId, setDropFolderId] = useState<string | null>(null);
  const [focusedNodeId, setFocusedNodeId] = useState<string | null>(null);
  const [pendingFocusNodeId, setPendingFocusNodeId] = useState<string | null>(null);
  const root = makeRootNode(activeSpace);
  const visibleTree = useMemo(
    () => projectVisibleTree(root.id, snapshots, expandedFolderIds),
    [expandedFolderIds, root.id, snapshots]
  );
  const getItemKey = useCallback(
    (index: number) => visibleTree.rows[index]?.key ?? index,
    [visibleTree.rows]
  );
  const draggedIndex = draggedNode
    ? visibleTree.rows.findIndex((row) => row.type === "node" && row.node.id === draggedNode.id)
    : -1;
  const focusedIndex = focusedNodeId
    ? visibleTree.rows.findIndex((row) => row.type === "node" && row.node.id === focusedNodeId)
    : -1;
  const rangeExtractor = useCallback((range: Range) => {
    const indexes = defaultRangeExtractor(range);
    for (const pinnedIndex of [draggedIndex, focusedIndex]) {
      if (pinnedIndex >= 0 && !indexes.includes(pinnedIndex)) indexes.push(pinnedIndex);
    }
    return indexes.sort((left, right) => left - right);
  }, [draggedIndex, focusedIndex]);
  const rowVirtualizer = useVirtualizer({
    count: visibleTree.rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => TREE_ROW_SIZE,
    getItemKey,
    overscan: TREE_OVERSCAN,
    rangeExtractor
  });
  const virtualItems = rowVirtualizer.getVirtualItems();
  const requestNodeFocus = useCallback((nodeId: string, index: number) => {
    setPendingFocusNodeId(nodeId);
    rowVirtualizer.scrollToIndex(index, { align: "auto" });
  }, [rowVirtualizer]);
  const focusLastNode = useCallback(() => {
    const index = findAdjacentNodeRowIndex(visibleTree.rows, visibleTree.rows.length, -1);
    const row = index === null ? undefined : visibleTree.rows[index];
    if (index === null || row?.type !== "node") return false;
    requestNodeFocus(row.node.id, index);
    return true;
  }, [requestNodeFocus, visibleTree.rows]);

  useLayoutEffect(() => {
    onTreeNavigationChange({ focusLastNode });
    return () => onTreeNavigationChange(null);
  }, [focusLastNode, onTreeNavigationChange]);

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

  useEffect(() => {
    if (pendingFocusNodeId === null) return;
    const pendingFocusIndex = visibleTree.rows.findIndex(
      (row) => row.type === "node" && row.node.id === pendingFocusNodeId
    );
    if (pendingFocusIndex < 0) {
      setPendingFocusNodeId(null);
      return;
    }
    const button = scrollRef.current?.querySelector<HTMLButtonElement>(
      `[data-tree-index="${pendingFocusIndex}"] [data-node-open]`
    );
    if (!button) {
      rowVirtualizer.scrollToIndex(pendingFocusIndex, { align: "auto" });
      return;
    }
    button.focus({ preventScroll: true });
    setPendingFocusNodeId(null);
  }, [pendingFocusNodeId, rowVirtualizer, virtualItems, visibleTree.rows]);

  function clearDrag() {
    setDraggedNode(null);
    setDropFolderId(null);
  }

  function handleDragOver(node: RestNode, event: DragEvent<HTMLDivElement>) {
    if (!canWriteActiveSpace || !draggedNode || node.kind !== "folder" || node.id === draggedNode.id) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDropFolderId(node.id);
  }

  function handleDrop(node: RestNode, event: DragEvent<HTMLDivElement>) {
    if (!canWriteActiveSpace || !draggedNode || node.kind !== "folder" || node.id === draggedNode.id) return;
    event.preventDefault();
    onMoveNodeToFolder(draggedNode, node);
    clearDrag();
  }

  function handleTreeKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const currentRow = (event.target as HTMLElement).closest("[data-tree-index]") as HTMLElement | null;
    const currentIndex = Number(currentRow?.dataset.treeIndex);
    if (!Number.isInteger(currentIndex)) return;
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const nextIndex = findAdjacentNodeRowIndex(visibleTree.rows, currentIndex, direction);
    if (nextIndex === null) return;
    const nextRow = visibleTree.rows[nextIndex];
    if (nextRow?.type !== "node") return;
    event.preventDefault();
    event.stopPropagation();
    requestNodeFocus(nextRow.node.id, nextIndex);
  }

  return (
    <>
      <div
        ref={scrollRef}
        role="tree"
        aria-label="Files"
        className="mt-2 min-h-0 flex-1 overflow-y-auto"
        onKeyDown={handleTreeKeyDown}
        onFocusCapture={(event) => {
          const row = (event.target as HTMLElement).closest("[data-tree-index]") as HTMLElement | null;
          const index = Number(row?.dataset.treeIndex);
          const treeRow = Number.isInteger(index) ? visibleTree.rows[index] : undefined;
          if (treeRow?.type === "node") setFocusedNodeId(treeRow.node.id);
        }}
        onBlurCapture={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocusedNodeId(null);
        }}
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
                className="absolute left-0 top-0 w-full pb-1"
                style={{ height: `${virtualRow.size}px`, transform: `translateY(${virtualRow.start}px)` }}
              >
                <VirtualTreeRow
                  row={row}
                  activeNodeId={activeNodeId}
                  dropFolderId={dropFolderId}
                  expandedFolderIds={expandedFolderIds}
                  fetchNextPage={row.type === "load-more" ? fetchNextPageByParent.current.get(row.parentId) : undefined}
                  scrollRef={scrollRef}
                  onToggleFolder={onToggleFolder}
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
      {visibleTree.queryParentIds.map((parentId) => (
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
  activeNodeId,
  dropFolderId,
  expandedFolderIds,
  fetchNextPage,
  scrollRef,
  onToggleFolder,
  onOpenNode,
  onNodeContextMenu,
  onDragStartNode,
  onDragOverNode,
  onDropOnNode,
  onDragEndNode,
  canWriteActiveSpace
}: {
  row: TreeRow;
  activeNodeId: string | null;
  dropFolderId: string | null;
  expandedFolderIds: ReadonlySet<string>;
  fetchNextPage?: () => void;
  scrollRef: RefObject<HTMLDivElement | null>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onNodeContextMenu: NodeContextHandler;
  onDragStartNode: (node: RestNode) => void;
  onDragOverNode: (node: RestNode, event: DragEvent<HTMLDivElement>) => void;
  onDropOnNode: (node: RestNode, event: DragEvent<HTMLDivElement>) => void;
  onDragEndNode: () => void;
  canWriteActiveSpace: boolean;
}) {
  if (row.type === "loading") {
    return <div role="status" className="flex h-8 items-center py-1 text-xs text-muted" style={{ paddingLeft: `${8 + row.depth * 14}px` }}>Loading…</div>;
  }
  if (row.type === "empty") {
    return <div role="status" className="flex h-8 items-center px-2 py-2 text-xs text-muted">No nodes yet.</div>;
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
      aria-selected={activeNodeId === node.id}
    >
      <NodeRow
        node={node}
        depth={row.depth}
        selected={activeNodeId === node.id}
        expanded={node.kind === "folder" && expandedFolderIds.has(node.id)}
        suffix={nodeMetaSuffix(node)}
        dropTarget={dropFolderId === node.id}
        onToggleFolder={onToggleFolder}
        onOpenNode={onOpenNode}
        onNodeContextMenu={onNodeContextMenu}
        onDragStartNode={canWriteActiveSpace ? onDragStartNode : undefined}
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
