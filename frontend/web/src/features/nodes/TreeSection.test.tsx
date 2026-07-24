import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode } from "../../entities/node/model";
import type { Space } from "../../entities/space/model";
import { TreeSection } from "./TreeSection";
import type { TreeKeyboardNavigation } from "./types";

const mocks = vi.hoisted(() => ({
  useNodeChildrenQuery: vi.fn(),
  scrollToIndex: vi.fn(),
  virtualStart: 0
}));

vi.mock("./useNodeQueries", () => ({ useNodeChildrenQuery: mocks.useNodeChildrenQuery }));
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count, getItemKey }: { count: number; getItemKey: (index: number) => string | number }) => ({
    getTotalSize: () => count * 36,
    getVirtualItems: () => Array.from({ length: Math.min(Math.max(count - mocks.virtualStart, 0), 20) }, (_, offset) => ({
      index: mocks.virtualStart + offset,
      key: getItemKey(mocks.virtualStart + offset),
      size: 36,
      start: (mocks.virtualStart + offset) * 36
    })),
    scrollToIndex: mocks.scrollToIndex
  })
}));

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
  beforeEach(() => {
    mocks.useNodeChildrenQuery.mockReset();
    mocks.scrollToIndex.mockReset();
    mocks.virtualStart = 0;
  });

  it("does not create child query observers for file rows", async () => {
    const file = node("file-1", "file");
    const rootQuery = query([file]);
    mocks.useNodeChildrenQuery.mockImplementation((_spaceId, nodeId) => nodeId === space.root_node_id ? rootQuery : query([]));

    renderTree(new Set());

    await waitFor(() => expect(queriedNodeIds()).toContain(space.root_node_id));
    expect(new Set(queriedNodeIds())).toEqual(new Set([space.root_node_id]));
  });

  it("creates child query observers only for expanded folders", async () => {
    const folder = node("folder-1", "folder");
    const child = node("text-1", "text", folder.id);
    const rootQuery = query([folder]);
    const folderQuery = query([child]);
    mocks.useNodeChildrenQuery.mockImplementation((_spaceId, nodeId) => {
      if (nodeId === space.root_node_id) return rootQuery;
      if (nodeId === folder.id) return folderQuery;
      return query([]);
    });

    renderTree(new Set([folder.id]));

    await waitFor(() => expect(queriedNodeIds()).toContain(folder.id));
    expect(new Set(queriedNodeIds())).toEqual(new Set([space.root_node_id, folder.id]));
  });

  it("renders only the rows returned by the virtualizer", async () => {
    const files = Array.from({ length: 1_000 }, (_, index) => node(`file-${index}`, "file"));
    const rootQuery = query(files);
    mocks.useNodeChildrenQuery.mockReturnValue(rootQuery);

    const view = renderTree(new Set());

    await waitFor(() => expect(view.container.querySelectorAll("[data-node-row]")).toHaveLength(20));
  });

  it("resolves a pending focus by node id after the projection changes", async () => {
    const files = Array.from({ length: 30 }, (_, index) => node(`file-${index}`, "file"));
    const rootQuery = query(files);
    mocks.useNodeChildrenQuery.mockReturnValue(rootQuery);
    let navigation: TreeKeyboardNavigation | null = null;
    const view = renderTree(new Set(), (next) => { navigation = next; });

    await waitFor(() => expect(navigation).not.toBeNull());
    act(() => expect(navigation?.focusLastNode()).toBe(true));
    expect(mocks.scrollToIndex).toHaveBeenCalledWith(29, { align: "auto" });

    rootQuery.data = { pages: [{ children: [node("inserted", "file"), ...files] }] };
    mocks.virtualStart = 11;
    view.rerender(treeElement(new Set(), (next) => { navigation = next; }));

    await waitFor(() => expect(view.getByRole("button", { name: "file-29.bin" })).toHaveFocus());
    expect(view.getByRole("button", { name: "file-28.bin" })).not.toHaveFocus();
  });
});

function renderTree(
  expandedFolderIds: Set<string>,
  onTreeNavigationChange: (navigation: TreeKeyboardNavigation | null) => void = vi.fn()
) {
  return render(treeElement(expandedFolderIds, onTreeNavigationChange));
}

function treeElement(
  expandedFolderIds: Set<string>,
  onTreeNavigationChange: (navigation: TreeKeyboardNavigation | null) => void
) {
  return (
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
      onTreeNavigationChange={onTreeNavigationChange}
      canWriteActiveSpace
    />
  );
}

function queriedNodeIds(): string[] {
  return mocks.useNodeChildrenQuery.mock.calls.map((call) => call[1] as string);
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
