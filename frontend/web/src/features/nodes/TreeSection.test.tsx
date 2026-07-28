import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode } from "../../api/types";
import { makeSpace } from "../../test/fixtures";
import { createTreeNodeFactory, treeSectionElement } from "../../test/treeSection";
import { resetTreeVirtualizer, setTreeVirtualizerStart, treeVirtualizerScrollToIndex } from "../../test/treeVirtualizer";
import type { TreeKeyboardNavigation } from "./types";

const mocks = vi.hoisted(() => ({
  useNodeChildrenQuery: vi.fn()
}));

vi.mock("./useNodeQueries", () => ({ useNodeChildrenQuery: mocks.useNodeChildrenQuery }));
vi.mock("./useTreeRestoreBatch", () => ({ useTreeRestoreBatch: () => false }));
vi.mock("@tanstack/react-virtual", () => import("../../test/treeVirtualizer"));

const space = makeSpace();
const node = createTreeNodeFactory(space);

describe("TreeSection", () => {
  beforeEach(() => {
    mocks.useNodeChildrenQuery.mockReset();
    resetTreeVirtualizer(20);
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

  it("inspects and toggles a folder without replacing the open editor node", async () => {
    const folder = node("folder-1", "folder");
    const rootQuery = query([folder]);
    const emptyQuery = query([]);
    mocks.useNodeChildrenQuery.mockImplementation((_spaceId, nodeId) =>
      nodeId === space.root_node_id ? rootQuery : emptyQuery
    );
    const onToggleFolder = vi.fn();
    const onInspectNode = vi.fn();
    const onOpenNode = vi.fn();
    const view = render(treeSectionElement(space, {
      openedNodeId: "open-text",
      onToggleFolder,
      onInspectNode,
      onOpenNode
    }));

    await waitFor(() => expect(view.getByRole("button", { name: folder.name })).toBeTruthy());
    fireEvent.click(view.getByRole("button", { name: folder.name }));

    expect(onInspectNode).toHaveBeenCalledWith(folder);
    expect(onToggleFolder).toHaveBeenCalledWith(folder.id);
    expect(onOpenNode).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("button", { name: `Expand ${folder.name}` }));

    expect(onInspectNode).toHaveBeenCalledTimes(2);
    expect(onToggleFolder).toHaveBeenCalledTimes(2);
    expect(onOpenNode).not.toHaveBeenCalled();
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
    expect(treeVirtualizerScrollToIndex).toHaveBeenCalledWith(29, { align: "auto" });

    rootQuery.data = { pages: [{ children: [node("inserted", "file"), ...files] }] };
    setTreeVirtualizerStart(11);
    view.rerender(treeSectionElement(space, {
      expandedFolderIds: new Set(),
      onTreeNavigationChange: (next) => { navigation = next; }
    }));

    await waitFor(() => expect(view.getByRole("button", { name: "file-29.bin" })).toHaveFocus());
    expect(view.getByRole("button", { name: "file-28.bin" })).not.toHaveFocus();
  });

  it("does not make effectively locked rows draggable", async () => {
    const locked = { ...node("text-1", "text"), effective_write_locked: true };
    mocks.useNodeChildrenQuery.mockReturnValue(query([locked]));

    const view = renderTree(new Set());

    const row = await findNodeRow(view, locked.name);
    expect(row).toHaveAttribute("draggable", "false");
  });

  it("blocks drops onto effectively locked folders", async () => {
    const source = node("text-1", "text");
    const destination = {
      ...node("folder-1", "folder"),
      effective_write_locked: true
    };
    const onMoveNodeToFolder = vi.fn();
    mocks.useNodeChildrenQuery.mockReturnValue(query([source, destination]));
    const view = render(treeSectionElement(space, { onMoveNodeToFolder }));

    const sourceRow = await findNodeRow(view, source.name);
    const destinationRow = await findNodeRow(view, destination.name);
    const dataTransfer = dragDataTransfer();
    fireEvent.dragStart(sourceRow, { dataTransfer });
    fireEvent.dragOver(destinationRow, { dataTransfer });
    fireEvent.drop(destinationRow, { dataTransfer });

    expect(onMoveNodeToFolder).not.toHaveBeenCalled();
  });

  it("moves an unlocked row into an unlocked folder", async () => {
    const source = node("text-1", "text");
    const destination = node("folder-1", "folder");
    const onMoveNodeToFolder = vi.fn();
    mocks.useNodeChildrenQuery.mockReturnValue(query([source, destination]));
    const view = render(treeSectionElement(space, { onMoveNodeToFolder }));

    const sourceRow = await findNodeRow(view, source.name);
    const destinationRow = await findNodeRow(view, destination.name);
    const dataTransfer = dragDataTransfer();
    fireEvent.dragStart(sourceRow, { dataTransfer });
    fireEvent.dragOver(destinationRow, { dataTransfer });
    fireEvent.drop(destinationRow, { dataTransfer });

    expect(onMoveNodeToFolder).toHaveBeenCalledWith(source, destination);
  });
});

function renderTree(
  expandedFolderIds: Set<string>,
  onTreeNavigationChange: (navigation: TreeKeyboardNavigation | null) => void = vi.fn()
) {
  return render(treeSectionElement(space, {
    expandedFolderIds,
    onTreeNavigationChange
  }));
}

async function findNodeRow(
  view: ReturnType<typeof render>,
  name: string
): Promise<HTMLElement> {
  const button = await waitFor(() => within(view.container).getByRole("button", { name }));
  const row = button.closest<HTMLElement>("[data-node-row]");
  if (!row) throw new Error(`missing row for ${name}`);
  return row;
}

function dragDataTransfer() {
  return {
    dropEffect: "none",
    effectAllowed: "none",
    setData: vi.fn()
  };
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
