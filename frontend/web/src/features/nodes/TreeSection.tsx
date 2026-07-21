import { ChevronsDownUp, Folder } from "lucide-react";
import type { DragEvent } from "react";
import { memo, useEffect, useRef, useState } from "react";

import type { RestNode, Space } from "../../api/types";
import { useNodeChildrenQuery } from "./useNodeQueries";
import { makeRootNode, nodeMetaSuffix } from "./nodeDisplay";
import { NodeRow } from "./NodeRow";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import type { NodeContextHandler } from "./types";

type TreeProps = {
  activeSpace: Space;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  draggedNode: RestNode | null;
  dropFolderId: string | null;
  onDragStartNode: (node: RestNode) => void;
  onDragOverNode: (node: RestNode, event: DragEvent<HTMLDivElement>) => void;
  onDropOnNode: (node: RestNode, event: DragEvent<HTMLDivElement>) => void;
  onDragEndNode: () => void;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onNodeContextMenu: NodeContextHandler;
  onMoveNodeToFolder: (node: RestNode, folder: RestNode) => void;
  canWriteActiveSpace: boolean;
};

export function TreeSection({ activeSpace, activeNodeId, expandedFolderIds, open, onToggle, onCollapseTree, onToggleFolder, onOpenNode, onNodeContextMenu, onMoveNodeToFolder, canWriteActiveSpace }: Omit<TreeProps, "draggedNode" | "dropFolderId" | "onDragStartNode" | "onDragOverNode" | "onDropOnNode" | "onDragEndNode"> & { open: boolean; onToggle: () => void; onCollapseTree: () => void }) {
  const [draggedNode, setDraggedNode] = useState<RestNode | null>(null);
  const [dropFolderId, setDropFolderId] = useState<string | null>(null);

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

  return (
    <section className="flex min-h-0 min-w-0 flex-col px-3 py-2">
      <SidebarSectionHeader icon={<Folder size={13} />} label="Files" open={open} onToggle={onToggle} action={{ label: "Collapse all folders", icon: <ChevronsDownUp size={13} />, onClick: onCollapseTree }} />
      {open ? (
        <div
          className="mt-2 min-h-0 flex-1 space-y-1 overflow-y-auto"
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropFolderId(null);
          }}
          onContextMenu={(event) => {
            if ((event.target as HTMLElement).closest("[data-node-row]")) return;
            onNodeContextMenu(makeRootNode(activeSpace), event);
          }}
        >
          <RootTree
            activeSpace={activeSpace}
            activeNodeId={activeNodeId}
            expandedFolderIds={expandedFolderIds}
            draggedNode={draggedNode}
            dropFolderId={dropFolderId}
            onDragStartNode={setDraggedNode}
            onDragOverNode={handleDragOver}
            onDropOnNode={handleDrop}
            onDragEndNode={clearDrag}
            onToggleFolder={onToggleFolder}
            onOpenNode={onOpenNode}
            onNodeContextMenu={onNodeContextMenu}
            onMoveNodeToFolder={onMoveNodeToFolder}
            canWriteActiveSpace={canWriteActiveSpace}
          />
        </div>
      ) : null}
    </section>
  );
}

function RootTree(props: TreeProps) {
  const root = makeRootNode(props.activeSpace);
  const childrenQuery = useNodeChildrenQuery(props.activeSpace.id, root.id, true);
  const children = childrenQuery.data?.pages.flatMap((page) => page.children) ?? [];

  return (
    <div>
      {childrenQuery.isLoading ? <div className="px-2 py-1 text-xs text-muted">Loading…</div> : null}
      {!childrenQuery.isLoading && children.length === 0 ? <div className="px-2 py-2 text-xs text-muted">No nodes yet.</div> : null}
      {children.map((child) => (
        <TreeNode key={child.id} node={child} depth={0} {...props} />
      ))}
      {childrenQuery.hasNextPage ? (
        <AutoLoadMore loaded={children.length} depth={0} isFetching={childrenQuery.isFetchingNextPage} fetchNextPage={() => childrenQuery.fetchNextPage()} />
      ) : null}
    </div>
  );
}

const TreeNode = memo(function TreeNode({ node, depth, ...props }: TreeProps & { node: RestNode; depth: number }) {
  const { activeNodeId, expandedFolderIds, dropFolderId, onDragStartNode, onDragOverNode, onDropOnNode, onDragEndNode, onToggleFolder, onOpenNode, onNodeContextMenu, canWriteActiveSpace } = props;
  const isExpanded = expandedFolderIds.has(node.id);
  return (
    <div>
      <NodeRow
        node={node}
        depth={depth}
        selected={activeNodeId === node.id}
        expanded={isExpanded}
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
      {node.kind === "folder" && isExpanded ? (
        <FolderChildren folder={node} depth={depth + 1} {...props} />
      ) : null}
    </div>
  );
});

function FolderChildren({ folder, depth, ...props }: TreeProps & { folder: RestNode; depth: number }) {
  const childrenQuery = useNodeChildrenQuery(props.activeSpace.id, folder.id, true);
  const children = childrenQuery.data?.pages.flatMap((page) => page.children) ?? [];
  return (
    <div>
      {childrenQuery.isLoading ? <div className="px-8 py-1 text-xs text-muted">Loading…</div> : null}
      {children.map((child) => <TreeNode key={child.id} node={child} depth={depth} {...props} />)}
      {childrenQuery.hasNextPage ? (
        <AutoLoadMore loaded={children.length} depth={depth} isFetching={childrenQuery.isFetchingNextPage} fetchNextPage={() => childrenQuery.fetchNextPage()} />
      ) : null}
    </div>
  );
}

function AutoLoadMore({ loaded, depth, isFetching, fetchNextPage }: { loaded: number; depth: number; isFetching: boolean; fetchNextPage: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const fetchRef = useRef(fetchNextPage);
  fetchRef.current = fetchNextPage;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !isFetching) fetchRef.current();
      },
      { rootMargin: "80px" }
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [isFetching]);
  return (
    <div ref={ref} className="py-1 text-xs text-faint" style={{ paddingLeft: `${8 + depth * 14}px` }}>
      {isFetching ? "Loading…" : `Scroll to load more (${loaded} loaded)`}
    </div>
  );
}
