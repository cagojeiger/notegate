import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { queryKeys } from "../../api/queryKeys";
import {
  createSpaceChangeSynchronizer,
  useSpaceChangeSync
} from "./useSpaceChangeSync";

const get = vi.fn();
const client = { get } as unknown as ApiClient;

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => client
}));

vi.mock("../../shared/hooks/usePageVisible", () => ({
  usePageVisible: () => true
}));

describe("useSpaceChangeSync", () => {
  afterEach(() => {
    get.mockReset();
  });

  it("establishes a baseline, then applies every returned change without a Space-wide refresh", async () => {
    get
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(12, [
        change(11, { node_id: "text-1", affected_parent_ids: ["parent-1"] }),
        change(12, { node_id: "text-2", affected_parent_ids: ["parent-2"] })
      ]));
    const queryClient = createQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    let renderCount = 0;

    renderHook(() => {
      renderCount += 1;
      useSpaceChangeSync("space-1");
    }, { wrapper: createWrapper(queryClient) });

    await waitForSignal(queryClient, 10);
    expect(invalidate).not.toHaveBeenCalled();
    const baselineRenderCount = renderCount;

    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    expect(invalidate).not.toHaveBeenCalled();
    expect(renderCount).toBe(baselineRenderCount);

    await refetchSignal(queryClient);
    await waitForSignal(queryClient, 12);
    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.children("space-1", "parent-1")
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.children("space-1", "parent-2")
      });
      expect(invalidate).not.toHaveBeenCalledWith({
        queryKey: queryKeys.space("space-1")
      });
    });
  });

  it("drops stale preview URLs when an external delete event is observed", async () => {
    get
      .mockResolvedValueOnce(response(20))
      .mockResolvedValueOnce(response(21, [change(21, {
        op_type: "item.delete",
        node_id: "file-1",
        item_kind: "file",
        affected_parent_ids: ["parent-1"]
      })]));
    const queryClient = createQueryClient();
    const previewKey = queryKeys.filePreviewUrl("space-1", "file-1");
    queryClient.setQueryData(previewKey, { url: "https://storage.example/stale" });

    renderHook(() => useSpaceChangeSync("space-1"), { wrapper: createWrapper(queryClient) });

    await waitForSignal(queryClient, 20);
    await refetchSignal(queryClient);
    await waitForSignal(queryClient, 21);

    await waitFor(() => expect(queryClient.getQueryData(previewKey)).toBeUndefined());
  });

  it("performs one bounded file-cache refresh when the sync token is no longer valid", async () => {
    get
      .mockResolvedValueOnce(response(30))
      .mockResolvedValueOnce({
        ...response(40),
        resync_required: true
      });
    const queryClient = createQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    renderHook(() => useSpaceChangeSync("space-1"), { wrapper: createWrapper(queryClient) });

    await waitForSignal(queryClient, 30);
    await refetchSignal(queryClient);
    await waitForSignal(queryClient, 40);

    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.childrenFamily("space-1")
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.nodes("space-1")
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.texts("space-1")
      });
      expect(invalidate).not.toHaveBeenCalledWith({
        queryKey: queryKeys.space("space-1")
      });
    });
  });

  it("serializes sync requests so an older response cannot overwrite a newer token", async () => {
    const first = deferred<ReturnType<typeof response>>();
    get
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(response(11, [change(11)]));
    const queryClient = createQueryClient();
    const sync = createSpaceChangeSynchronizer(client, queryClient);

    const older = sync("space-1");
    const newer = sync("space-1");

    await waitFor(() => expect(get).toHaveBeenCalledTimes(1));
    first.resolve(response(10));
    await older;
    await newer;

    expect(get).toHaveBeenCalledTimes(2);
    expect(get.mock.calls[1]?.[0]).toContain("after_id=10");
  });
});

function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false } }
  });
}

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

async function refetchSignal(queryClient: QueryClient) {
  await queryClient.refetchQueries({
    queryKey: queryKeys.spaceChangeSignal("space-1"),
    exact: true
  });
}

async function waitForSignal(queryClient: QueryClient, eventId: number) {
  await waitFor(() => {
    const signal = queryClient.getQueryData<ReturnType<typeof response>>(
      queryKeys.spaceChangeSignal("space-1")
    );
    expect(signal?.next_after_id).toBe(eventId);
  });
}

function response(
  nextAfterId: number,
  changes: ReturnType<typeof change>[] = []
) {
  return {
    changes,
    next_after_id: nextAfterId,
    has_more: false,
    resync_required: false
  };
}

function change(
  id: number,
  overrides: Partial<{
    op_type: string;
    node_id: string | null;
    item_kind: "folder" | "text" | "file" | null;
    affected_parent_ids: string[];
    parent_scope_known: boolean;
    path_changed: boolean;
    subtree_changed: boolean;
  }> = {}
) {
  return {
    id,
    node_id: "node-1",
    op_type: "text.write",
    item_kind: "text" as const,
    affected_parent_ids: ["parent-1"],
    parent_scope_known: true,
    path_changed: false,
    subtree_changed: false,
    ...overrides
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
