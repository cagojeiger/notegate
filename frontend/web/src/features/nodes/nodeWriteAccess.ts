import type { NodeSummary, RestNode } from "../../api/types";

type NodeWriteState = Pick<NodeSummary, "effective_write_locked">;
type MutableNode = Pick<NodeSummary, "parent_id" | "effective_write_locked">;
type FolderNode = Pick<NodeSummary, "kind" | "effective_write_locked">;
type MovableNode = Pick<NodeSummary, "id" | "parent_id" | "kind" | "effective_write_locked">;
type CreateSource = Pick<
  RestNode,
  "id" | "parent_id" | "path" | "kind" | "effective_write_locked" | "write_lock_sources"
>;

export type NodeCreateTarget = {
  id: string;
  path: string;
  writeLocked: boolean;
};

export function canWriteNode(node: NodeWriteState, canWrite: boolean): boolean {
  return canWrite && !node.effective_write_locked;
}

export function canMutateNode(node: MutableNode, canWrite: boolean): boolean {
  return node.parent_id !== null && canWriteNode(node, canWrite);
}

export function canCreateInFolder(folder: FolderNode, canWrite: boolean): boolean {
  return folder.kind === "folder" && canWriteNode(folder, canWrite);
}

export function canMoveNodeToFolder(
  node: MovableNode,
  folder: MovableNode,
  canWrite: boolean
): boolean {
  return node.id !== folder.id
    && canMutateNode(node, canWrite)
    && canCreateInFolder(folder, canWrite);
}

export function resolveNodeCreateTarget(
  rootNodeId: string,
  activeNode: CreateSource | null
): NodeCreateTarget {
  if (!activeNode) {
    return { id: rootNodeId, path: "/", writeLocked: false };
  }
  if (activeNode.kind === "folder") {
    return {
      id: activeNode.id,
      path: activeNode.path,
      writeLocked: activeNode.effective_write_locked
    };
  }
  return {
    id: activeNode.parent_id ?? rootNodeId,
    path: parentPath(activeNode.path),
    writeLocked: activeNode.write_lock_sources.some((source) => source.node_id !== activeNode.id)
  };
}

function parentPath(path: string): string {
  const lastSlash = path.lastIndexOf("/");
  return lastSlash <= 0 ? "/" : path.slice(0, lastSlash);
}
