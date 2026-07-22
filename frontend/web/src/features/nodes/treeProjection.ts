import type { RestNode } from "../../api/types";

export type TreeFolderSnapshot = {
  children: RestNode[];
  isLoading: boolean;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
};

export type TreeRow =
  | { type: "node"; key: string; node: RestNode; depth: number }
  | { type: "loading"; key: string; parentId: string; depth: number }
  | { type: "load-more"; key: string; parentId: string; depth: number; loaded: number; isFetching: boolean }
  | { type: "empty"; key: string; depth: number };

export type VisibleTree = {
  rows: TreeRow[];
  queryParentIds: string[];
};

export function findAdjacentNodeRowIndex(
  rows: readonly TreeRow[],
  currentIndex: number,
  direction: -1 | 1
): number | null {
  for (let index = currentIndex + direction; index >= 0 && index < rows.length; index += direction) {
    if (rows[index]?.type === "node") return index;
  }
  return null;
}

export function projectVisibleTree(
  rootId: string,
  snapshots: ReadonlyMap<string, TreeFolderSnapshot>,
  expandedFolderIds: ReadonlySet<string>
): VisibleTree {
  const rows: TreeRow[] = [];
  const queryParentIds = [rootId];

  function appendChildren(parentId: string, depth: number, isRoot = false) {
    const snapshot = snapshots.get(parentId);
    if (!snapshot) {
      rows.push({ type: "loading", key: `loading:${parentId}`, parentId, depth });
      return;
    }

    if (snapshot.isLoading && snapshot.children.length === 0) {
      rows.push({ type: "loading", key: `loading:${parentId}`, parentId, depth });
      return;
    }

    if (isRoot && snapshot.children.length === 0) {
      rows.push({ type: "empty", key: `empty:${parentId}`, depth });
    }

    for (const node of snapshot.children) {
      rows.push({ type: "node", key: `node:${node.id}`, node, depth });
      if (node.kind === "folder" && expandedFolderIds.has(node.id)) {
        queryParentIds.push(node.id);
        appendChildren(node.id, depth + 1);
      }
    }

    if (snapshot.hasNextPage) {
      rows.push({
        type: "load-more",
        key: `load-more:${parentId}`,
        parentId,
        depth,
        loaded: snapshot.children.length,
        isFetching: snapshot.isFetchingNextPage
      });
    }
  }

  appendChildren(rootId, 0, true);
  return { rows, queryParentIds };
}
