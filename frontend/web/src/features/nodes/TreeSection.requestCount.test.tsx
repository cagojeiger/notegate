import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { applyExternalFileChanges } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { ChildrenResponse, RestNode, Space } from "../../api/types";
import { TreeSection } from "./TreeSection";
import { useTreeRestoreBatch } from "./useTreeRestoreBatch";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn()
}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({ get: mocks.get, post: mocks.post } as unknown as ApiClient)
}));

vi.mock("@tanstack/react-virtual", () => ({
  defaultRangeExtractor: ({ startIndex, endIndex }: { startIndex: number; endIndex: number }) =>
    Array.from({ length: endIndex - startIndex + 1 }, (_, index) => startIndex + index),
  useVirtualizer: ({ count, getItemKey }: { count: number; getItemKey: (index: number) => string | number }) => ({
    getTotalSize: () => count * 36,
    getVirtualItems: () => Array.from({ length: count }, (_, index) => ({
      index,
      key: getItemKey(index),
      size: 36,
      start: index * 36
    })),
    scrollToIndex: vi.fn()
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

describe("TreeSection request count", () => {
  beforeEach(() => {
    mocks.get.mockReset();
    mocks.post.mockReset();
  });

  it("restores ten expanded folders from a fresh in-memory cache without HTTP requests", async () => {
    const folders = Array.from({ length: 10 }, (_, index) =>
      node(`folder-${index}`, "folder", space.root_node_id)
    );
    const queryClient = createQueryClient();
    seedChildren(queryClient, space.root_node_id, folders);
    folders.forEach((folder) => seedChildren(queryClient, folder.id, []));

    renderTree(queryClient, new Set(folders.map((folder) => folder.id)));

    await waitFor(() =>
      expect(document.querySelectorAll("[data-node-row]")).toHaveLength(10)
    );
    expect(mocks.get).not.toHaveBeenCalled();
    expect(mocks.post).not.toHaveBeenCalled();
  });

  it("restores ten expanded folders with one bounded batch request", async () => {
    const folders = Array.from({ length: 10 }, (_, index) =>
      node(`folder-${index}`, "folder", space.root_node_id)
    );
    mocks.post.mockImplementation(
      (_path: string, body: { parent_ids: string[] }) =>
        Promise.resolve(readyBatchResponse(folders, body.parent_ids))
    );

    const view = renderTree(
      createQueryClient(),
      new Set(folders.map((folder) => folder.id))
    );

    await waitFor(() =>
      expect(view.container.querySelectorAll("[data-node-row]")).toHaveLength(10)
    );
    expect(mocks.post).toHaveBeenCalledOnce();
    expect(mocks.get).not.toHaveBeenCalled();
  });

  it("reaches a cached three-level target without sequential HTTP requests", async () => {
    const first = node("folder-1", "folder", space.root_node_id);
    const second = node("folder-2", "folder", first.id, "/folder-1/folder-2");
    const third = node("folder-3", "folder", second.id, "/folder-1/folder-2/folder-3");
    const target = node("target", "text", third.id, "/folder-1/folder-2/folder-3/target.md");
    const queryClient = createQueryClient();
    seedChildren(queryClient, space.root_node_id, [first]);
    seedChildren(queryClient, first.id, [second]);
    seedChildren(queryClient, second.id, [third]);
    seedChildren(queryClient, third.id, [target]);

    const view = renderTree(
      queryClient,
      new Set([first.id, second.id, third.id])
    );

    await waitFor(() =>
      expect(view.getByRole("button", { name: "target.md" })).toBeInTheDocument()
    );
    expect(mocks.get).not.toHaveBeenCalled();
    expect(mocks.post).not.toHaveBeenCalled();
  });

  it("partitions large restores into bounded batch requests", async () => {
    const folders = Array.from({ length: 20 }, (_, index) =>
      node(`folder-${index}`, "folder", space.root_node_id)
    );
    mocks.post.mockImplementation(
      (_path: string, body: { parent_ids: string[] }) =>
        Promise.resolve(readyBatchResponse(folders, body.parent_ids))
    );

    const view = renderTree(
      createQueryClient(),
      new Set(folders.map((folder) => folder.id))
    );

    await waitFor(() =>
      expect(view.container.querySelectorAll("[data-node-row]")).toHaveLength(20)
    );
    expect(mocks.post).toHaveBeenCalledTimes(2);
    expect(
      mocks.post.mock.calls.every(
        ([, body]) => (body as { parent_ids: string[] }).parent_ids.length <= 16
      )
    ).toBe(true);
    expect(mocks.get).not.toHaveBeenCalled();
  });

  it("falls back to folder queries when the restore batch fails", async () => {
    const folders = Array.from({ length: 10 }, (_, index) =>
      node(`folder-${index}`, "folder", space.root_node_id)
    );
    mocks.post.mockRejectedValue(new Error("batch unavailable"));
    mocks.get.mockImplementation((path: string) =>
      Promise.resolve(
        childrenResponse(
          path.includes(`/${space.root_node_id}/`) ? folders : []
        )
      )
    );

    const view = renderTree(
      createQueryClient(),
      new Set(folders.map((folder) => folder.id))
    );

    await waitFor(() =>
      expect(view.container.querySelectorAll("[data-node-row]")).toHaveLength(10)
    );
    expect(mocks.post).toHaveBeenCalledOnce();
    const requestedParentIds = mocks.get.mock.calls.map(([path]) =>
      (path as string).match(/\/nodes\/([^/]+)\/children/)?.[1]
    );
    expect(new Set(requestedParentIds)).toEqual(
      new Set([space.root_node_id, ...folders.map((folder) => folder.id)])
    );
  });

  it("retries a failed restore batch after the children revision advances", async () => {
    const folder = node("folder-1", "folder", space.root_node_id);
    mocks.post
      .mockRejectedValueOnce(new Error("batch unavailable"))
      .mockImplementation(
        (_path: string, body: { parent_ids: string[] }) =>
          Promise.resolve(readyBatchResponse([folder], body.parent_ids))
      );
    const queryClient = createQueryClient();

    renderHook(
      () =>
        useTreeRestoreBatch(
          space.id,
          space.root_node_id,
          new Set([folder.id])
        ),
      { wrapper: wrapper(queryClient) }
    );

    await waitFor(() => expect(mocks.post).toHaveBeenCalledOnce());
    act(() => {
      queryClient.setQueryData(queryKeys.childrenRevision(space.id), 1);
    });

    await waitFor(() => expect(mocks.post).toHaveBeenCalledTimes(2));
  });

  it("discards a restore batch invalidated while it is in flight", async () => {
    const staleFolder = node("folder-1", "folder", space.root_node_id);
    const freshFolder = { ...staleFolder, name: "fresh-folder", path: "/fresh-folder" };
    let resolveBatch: ((value: ReturnType<typeof readyBatchResponse>) => void) | null = null;
    mocks.post.mockImplementation(
      () =>
        new Promise<ReturnType<typeof readyBatchResponse>>((resolve) => {
          resolveBatch = resolve;
        })
    );
    mocks.get.mockImplementation((path: string) =>
      Promise.resolve(
        childrenResponse(
          path.includes(`/${space.root_node_id}/`) ? [freshFolder] : []
        )
      )
    );
    const queryClient = createQueryClient();
    const view = renderTree(queryClient, new Set([staleFolder.id]));
    await waitFor(() => expect(mocks.post).toHaveBeenCalledOnce());

    await applyExternalFileChanges(queryClient, space.id, [{
      id: 12,
      node_id: staleFolder.id,
      op_type: "item.update",
      item_kind: "folder",
      affected_parent_ids: [space.root_node_id],
      parent_scope_known: true,
      path_changed: false,
      subtree_changed: false
    }]);
    await act(async () => {
      resolveBatch?.(readyBatchResponse([staleFolder], [
        space.root_node_id,
        staleFolder.id
      ]));
    });

    await waitFor(() =>
      expect(view.getByRole("button", { name: "fresh-folder" })).toBeInTheDocument()
    );
    expect(view.queryByRole("button", { name: staleFolder.name })).not.toBeInTheDocument();
    expect(mocks.get).toHaveBeenCalled();
  });

  it("does not refetch a visible branch for an external change under a collapsed branch", async () => {
    const visible = node("visible", "folder", space.root_node_id);
    const hidden = node("hidden", "folder", space.root_node_id);
    const queryClient = createQueryClient();
    seedChildren(queryClient, space.root_node_id, [visible, hidden]);
    seedChildren(queryClient, visible.id, [node("visible-file", "text", visible.id)]);
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    renderTree(queryClient, new Set([visible.id]));
    await waitFor(() =>
      expect(document.querySelectorAll("[data-node-row]")).toHaveLength(3)
    );
    mocks.get.mockClear();

    await applyExternalFileChanges(queryClient, space.id, [{
      id: 11,
      node_id: "hidden-file",
      op_type: "text.write",
      item_kind: "text",
      affected_parent_ids: [hidden.id],
      parent_scope_known: true,
      path_changed: false,
      subtree_changed: false
    }]);

    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.children(space.id, hidden.id)
    });
    expect(resetQueries).not.toHaveBeenCalledWith({
      queryKey: queryKeys.children(space.id, visible.id)
    });
    expect(mocks.get).not.toHaveBeenCalled();
  });
});

function renderTree(queryClient: QueryClient, expandedFolderIds: Set<string>) {
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
      onTreeNavigationChange={vi.fn()}
      canWriteActiveSpace
    />,
    { wrapper: wrapper(queryClient) }
  );
}

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        staleTime: 5_000
      }
    }
  });
}

function seedChildren(
  queryClient: QueryClient,
  parentId: string,
  children: RestNode[]
) {
  queryClient.setQueryData(queryKeys.children(space.id, parentId), {
    pages: [childrenResponse(children, parentId)],
    pageParams: [null]
  });
}

function childrenResponse(
  children: RestNode[],
  parentId = space.root_node_id
): ChildrenResponse {
  return {
    parent: {
      id: parentId,
      path: "/"
    },
    children,
    page: {
      next_cursor: null,
      has_more: false,
      limit: 100,
      returned: children.length
    }
  };
}

function readyBatchResponse(folders: RestNode[], parentIds: string[]) {
  return {
    results: parentIds.map((parentId) => ({
      parent_id: parentId,
      status: "ready" as const,
      parent: { id: parentId, path: "/" },
      children: parentId === space.root_node_id ? folders : [],
      page: {
        next_cursor: null,
        has_more: false,
        limit: 100,
        returned: parentId === space.root_node_id ? folders.length : 0
      }
    }))
  };
}

function node(
  id: string,
  kind: RestNode["kind"],
  parentId: string,
  path = `/${id}${kind === "text" ? ".md" : ""}`
): RestNode {
  const name = kind === "text" ? `${id}.md` : id;
  return {
    id,
    space_id: space.id,
    parent_id: parentId,
    name,
    kind,
    path,
    sort_order: 0,
    metadata: {},
    has_children: kind === "folder",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-01T00:00:00Z"
  };
}
