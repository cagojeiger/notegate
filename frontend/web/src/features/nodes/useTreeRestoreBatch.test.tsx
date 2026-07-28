import { QueryCache, QueryClient, QueryClientProvider, type QueryClientConfig } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { MAX_BATCH_CHILDREN_PARENTS } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { BatchChildrenItem, BatchChildrenResponse } from "../../api/types";
import { createMockApiClient } from "../../test/apiClient";
import { makeSpace } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { useTreeRestoreBatch } from "./useTreeRestoreBatch";

const apiClientState = vi.hoisted((): { client: ApiClient | null } => ({ client: null }));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => apiClientState.client!
}));

const client = createMockApiClient();
apiClientState.client = client;
const space = makeSpace();
const folderIds = ["folder-a", "folder-b"] as const;

describe("useTreeRestoreBatch", () => {
  beforeEach(() => {
    client.post.mockReset();
  });

  it.each([
    ["all parents are cached", true],
    ["only one parent is missing", false]
  ])("skips batching when %s", (_label, cacheFolder) => {
    const queryClient = createTestQueryClient();
    seedChildren(queryClient, space.root_node_id);
    if (cacheFolder) seedChildren(queryClient, "folder-1");
    const { result } = renderRestore(queryClient, ["folder-1"]);

    expect(result.current).toBe(false);
    expect(client.post).not.toHaveBeenCalled();
  });

  it("hydrates missing parents in stable order without replacing cached parents", async () => {
    mockReadyBatch();
    const { queryClient, result } = renderSeededRestore([folderIds[1], folderIds[0]]);
    const cachedRoot = cachedChildren(queryClient, space.root_node_id);

    expect(result.current).toBe(true);
    await waitFor(() => expect(result.current).toBe(false));
    expect(client.post).toHaveBeenCalledWith(BATCH_PATH, {
      parent_ids: ["folder-a", "folder-b"],
      limit: 100
    });
    expect(cachedChildren(queryClient, space.root_node_id)).toBe(cachedRoot);
    expectChildrenCached(queryClient, ...folderIds);
  });

  it("partitions large restores at the API parent limit", async () => {
    const folders = Array.from(
      { length: MAX_BATCH_CHILDREN_PARENTS + 1 },
      (_, index) => `folder-${index.toString().padStart(2, "0")}`
    );
    mockReadyBatch();
    const { queryClient, result } = renderSeededRestore(folders);

    await waitFor(() => expect(result.current).toBe(false));
    expect(client.post).toHaveBeenNthCalledWith(1, BATCH_PATH, {
      parent_ids: folders.slice(0, MAX_BATCH_CHILDREN_PARENTS),
      limit: 100
    });
    expect(client.post).toHaveBeenNthCalledWith(2, BATCH_PATH, {
      parent_ids: folders.slice(MAX_BATCH_CHILDREN_PARENTS),
      limit: 100
    });
    expect(client.post).toHaveBeenCalledTimes(2);
    expectChildrenCached(queryClient, folders[folders.length - 1]!);
  });

  it("retries a failed batch only after the children revision advances", async () => {
    client.post
      .mockRejectedValueOnce(new Error("batch unavailable"))
      .mockImplementation(readyBatchRequest);
    const { queryClient, ...view } = renderSeededRestore(folderIds);

    await waitFor(() => expect(view.result.current).toBe(false));
    view.rerender();
    expect(client.post).toHaveBeenCalledOnce();

    act(() => {
      queryClient.setQueryData(queryKeys.childrenRevision(space.id), 1);
    });

    await waitFor(() => expect(client.post).toHaveBeenCalledTimes(2));
    await waitFor(() => expectChildrenCached(queryClient, "folder-a"));
  });

  it("does not hydrate a batch invalidated while it is in flight", async () => {
    const batch = deferred<BatchChildrenResponse>();
    client.post.mockReturnValue(batch.promise);
    const { queryClient, result } = renderSeededRestore(folderIds);
    await waitFor(() => expect(client.post).toHaveBeenCalledOnce());
    expect(result.current).toBe(true);

    act(() => {
      queryClient.setQueryData(queryKeys.childrenRevision(space.id), 1);
    });
    await act(async () => {
      batch.resolve(readyBatchResponse(folderIds));
    });

    await waitFor(() => expect(result.current).toBe(false));
    expect(client.post).toHaveBeenCalledOnce();
    expectChildrenMissing(queryClient, "folder-a");
  });

  it("accepts terminal missing-parent statuses without caching them", async () => {
    const onError = vi.fn();
    client.post.mockResolvedValue({
      results: [
        terminalResult(folderIds[0], "not_found"),
        terminalResult(folderIds[1], "not_folder")
      ]
    });
    const { queryClient, result } = renderSeededRestore(folderIds, {
      queryCache: new QueryCache({ onError })
    });

    await waitFor(() => expect(result.current).toBe(false));
    expect(client.post).toHaveBeenCalledOnce();
    expect(onError).not.toHaveBeenCalled();
    expectChildrenMissing(queryClient, ...folderIds);
  });

  it.each([
    ["a missing result", readyBatchResponse([folderIds[0]]), "does not match the request"],
    ["out-of-order results", readyBatchResponse([folderIds[1], folderIds[0]]), "does not match the request"],
    ["an unknown status", unknownStatusResponse(), "unknown status"],
    ["a ready result without a parent", incompleteReadyResponse("parent"), "incomplete"],
    ["a ready result without a page", incompleteReadyResponse("page"), "incomplete"]
  ])("rejects %s", async (_label, response, message) => {
    const onError = vi.fn();
    client.post.mockResolvedValue(response);
    const { queryClient, result } = renderSeededRestore(folderIds, {
      queryCache: new QueryCache({ onError })
    });

    await waitFor(() => expect(result.current).toBe(false));
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: expect.stringContaining(message) }),
      expect.anything()
    );
    expectChildrenMissing(queryClient, folderIds[0]);
  });
});

const BATCH_PATH = `/api/v1/spaces/${space.id}/nodes:batchListChildren`;

function renderSeededRestore(folderIds: readonly string[], config?: QueryClientConfig) {
  const queryClient = createTestQueryClient(config);
  seedChildren(queryClient, space.root_node_id);
  return { queryClient, ...renderRestore(queryClient, folderIds) };
}

function renderRestore(queryClient: QueryClient, folderIds: readonly string[]) {
  const expanded = new Set(folderIds);
  return renderHook(
    () => useTreeRestoreBatch(space.id, space.root_node_id, expanded),
    { wrapper: ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    ) }
  );
}

function seedChildren(queryClient: QueryClient, parentId: string) {
  queryClient.setQueryData(
    queryKeys.children(space.id, parentId),
    childrenCache(parentId)
  );
}

function cachedChildren(queryClient: QueryClient, parentId: string) {
  return queryClient.getQueryData(queryKeys.children(space.id, parentId));
}

function expectChildrenCached(queryClient: QueryClient, ...parentIds: string[]) {
  for (const parentId of parentIds) {
    expect(cachedChildren(queryClient, parentId)).toEqual(childrenCache(parentId));
  }
}

function expectChildrenMissing(queryClient: QueryClient, ...parentIds: string[]) {
  for (const parentId of parentIds) {
    expect(cachedChildren(queryClient, parentId)).toBeUndefined();
  }
}

function childrenCache(parentId: string) {
  return {
    pages: [{
      parent: { id: parentId, path: "/" },
      children: [],
      page: { next_cursor: null, has_more: false, limit: 100, returned: 0 }
    }], pageParams: [null]
  };
}

function readyBatchResponse(parentIds: readonly string[]): BatchChildrenResponse {
  return { results: parentIds.map(readyResult) };
}

function readyResult(parentId: string): BatchChildrenItem {
  const page = childrenCache(parentId).pages[0];
  return { parent_id: parentId, status: "ready", ...page };
}

function terminalResult(
  parentId: string,
  status: "not_found" | "not_folder"
): BatchChildrenItem {
  return { parent_id: parentId, status, parent: null, children: [], page: null };
}

function parentIds(body: unknown): string[] {
  return (body as { parent_ids: string[] }).parent_ids;
}

function mockReadyBatch() {
  client.post.mockImplementation(readyBatchRequest);
}

function readyBatchRequest(_path: string, body: unknown) {
  return Promise.resolve(readyBatchResponse(parentIds(body)));
}

function unknownStatusResponse() {
  return {
    results: [{ ...readyResult(folderIds[0]), status: "pending" }, readyResult(folderIds[1])]
  };
}

function incompleteReadyResponse(field: "parent" | "page"): BatchChildrenResponse {
  const result = readyResult(folderIds[0]);
  return { results: [{ ...result, [field]: null }, readyResult(folderIds[1])] };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}
