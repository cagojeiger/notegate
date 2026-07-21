import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode, Space } from "../../api/types";
import { TreeSection } from "./TreeSection";

const mocks = vi.hoisted(() => ({ useNodeChildrenQuery: vi.fn() }));

vi.mock("./useNodeQueries", () => ({ useNodeChildrenQuery: mocks.useNodeChildrenQuery }));

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

describe("TreeSection", () => {
  beforeEach(() => mocks.useNodeChildrenQuery.mockReset());

  it("does not create child query observers for file rows", () => {
    const file = node("file-1", "file");
    mocks.useNodeChildrenQuery.mockImplementation((_spaceId, nodeId) => query(nodeId === space.root_node_id ? [file] : []));

    renderTree(new Set());

    expect(mocks.useNodeChildrenQuery).toHaveBeenCalledTimes(1);
    expect(mocks.useNodeChildrenQuery).toHaveBeenCalledWith(space.id, space.root_node_id, true);
  });

  it("creates child query observers only for expanded folders", () => {
    const folder = node("folder-1", "folder");
    const child = node("text-1", "text", folder.id);
    mocks.useNodeChildrenQuery.mockImplementation((_spaceId, nodeId) => {
      if (nodeId === space.root_node_id) return query([folder]);
      if (nodeId === folder.id) return query([child]);
      return query([]);
    });

    renderTree(new Set([folder.id]));

    expect(mocks.useNodeChildrenQuery).toHaveBeenCalledTimes(2);
    expect(mocks.useNodeChildrenQuery).toHaveBeenNthCalledWith(2, space.id, folder.id, true);
  });
});

function renderTree(expandedFolderIds: Set<string>) {
  return render(
    <TreeSection
      activeSpace={space}
      activeNodeId={null}
      expandedFolderIds={expandedFolderIds}
      open
      onToggle={vi.fn()}
      onCollapseTree={vi.fn()}
      onToggleFolder={vi.fn()}
      onOpenNode={vi.fn()}
      onNodeContextMenu={vi.fn()}
      onMoveNodeToFolder={vi.fn()}
      canWriteActiveSpace
    />
  );
}

function query(children: RestNode[]) {
  return {
    data: { pages: [{ children }] },
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn()
  };
}

function node(id: string, kind: RestNode["kind"], parentId = space.root_node_id): RestNode {
  const name = kind === "folder" ? id : `${id}.${kind === "text" ? "md" : "bin"}`;
  return {
    id,
    space_id: space.id,
    parent_id: parentId,
    name,
    kind,
    path: `/${name}`,
    sort_order: 0,
    metadata: {},
    has_children: kind === "folder",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-01T00:00:00Z"
  };
}
