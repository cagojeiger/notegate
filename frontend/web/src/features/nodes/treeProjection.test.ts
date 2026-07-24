import { describe, expect, it } from "vitest";

import type { RestNode } from "../../entities/node/model";
import { findAdjacentNodeRowIndex, projectVisibleTree, type TreeFolderSnapshot, type TreeRow } from "./treeProjection";

describe("projectVisibleTree", () => {
  it("projects expanded folders in display order", () => {
    const folder = node("folder", "folder", "root");
    const sibling = node("sibling", "text", "root");
    const child = node("child", "text", folder.id);
    const snapshots = new Map<string, TreeFolderSnapshot>([
      ["root", snapshot([folder, sibling])],
      [folder.id, snapshot([child])]
    ]);

    const result = projectVisibleTree("root", snapshots, new Set([folder.id]));

    expect(result.rows.map((row) => row.key)).toEqual([
      `node:${folder.id}`,
      `node:${child.id}`,
      `node:${sibling.id}`
    ]);
    expect(result.rows.map((row) => row.depth)).toEqual([0, 1, 0]);
    expect(result.queryParentIds).toEqual(["root", folder.id]);
  });

  it("does not query folders hidden behind a collapsed ancestor", () => {
    const parent = node("parent", "folder", "root");
    const child = node("child", "folder", parent.id);
    const snapshots = new Map<string, TreeFolderSnapshot>([
      ["root", snapshot([parent])],
      [parent.id, snapshot([child])]
    ]);

    const result = projectVisibleTree("root", snapshots, new Set([child.id]));

    expect(result.rows.map((row) => row.key)).toEqual([`node:${parent.id}`]);
    expect(result.queryParentIds).toEqual(["root"]);
  });

  it("adds loading and pagination rows at their tree depth", () => {
    const folder = node("folder", "folder", "root");
    const snapshots = new Map<string, TreeFolderSnapshot>([
      ["root", snapshot([folder], { hasNextPage: true, isFetchingNextPage: true })]
    ]);

    const result = projectVisibleTree("root", snapshots, new Set([folder.id]));

    expect(result.rows).toEqual([
      { type: "node", key: `node:${folder.id}`, node: folder, depth: 0 },
      { type: "loading", key: `loading:${folder.id}`, parentId: folder.id, depth: 1 },
      {
        type: "load-more",
        key: "load-more:root",
        parentId: "root",
        depth: 0,
        loaded: 1,
        isFetching: true
      }
    ]);
    expect(result.queryParentIds).toEqual(["root", folder.id]);
  });

  it("uses a single empty row for an empty root", () => {
    const result = projectVisibleTree("root", new Map([["root", snapshot([])]]), new Set());

    expect(result.rows).toEqual([{ type: "empty", key: "empty:root", depth: 0 }]);
  });
});

describe("findAdjacentNodeRowIndex", () => {
  const rows: TreeRow[] = [
    { type: "node", key: "node:first", node: node("first", "text", "root"), depth: 0 },
    { type: "loading", key: "loading:folder", parentId: "folder", depth: 1 },
    { type: "load-more", key: "load-more:root", parentId: "root", depth: 0, loaded: 1, isFetching: false },
    { type: "node", key: "node:last", node: node("last", "text", "root"), depth: 0 }
  ];

  it("skips non-node rows in both directions", () => {
    expect(findAdjacentNodeRowIndex(rows, 0, 1)).toBe(3);
    expect(findAdjacentNodeRowIndex(rows, 3, -1)).toBe(0);
  });

  it("returns null at the tree boundaries", () => {
    expect(findAdjacentNodeRowIndex(rows, 0, -1)).toBeNull();
    expect(findAdjacentNodeRowIndex(rows, 3, 1)).toBeNull();
  });
});

function snapshot(
  children: RestNode[],
  overrides: Partial<TreeFolderSnapshot> = {}
): TreeFolderSnapshot {
  return {
    children,
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    ...overrides
  };
}

function node(id: string, kind: RestNode["kind"], parentId: string): RestNode {
  return {
    id,
    space_id: "space",
    parent_id: parentId,
    name: id,
    kind,
    path: `/${id}`,
    sort_order: 0,
    metadata: {},
    has_children: kind === "folder",
    created_by: { id: "user", kind: "user", display_name: "User" },
    updated_by: { id: "user", kind: "user", display_name: "User" },
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-01T00:00:00Z"
  };
}
