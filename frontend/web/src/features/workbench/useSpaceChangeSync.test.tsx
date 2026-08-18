import {
  focusManager,
  onlineManager,
  QueryClientProvider,
  type QueryClient
} from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import { createMockApiClient } from "../../test/apiClient";
import { createTestQueryClient } from "../../test/queryClient";
import {
  createSpaceChangePollingBackoff,
  createSpaceChangeSynchronizer,
  useSpaceChangeSync
} from "./useSpaceChangeSync";

const apiClientState = vi.hoisted(
  (): { client: ApiClient | null } => ({ client: null })
);
const pageVisibility = vi.hoisted(() => ({ visible: true }));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => apiClientState.client!
}));

vi.mock("../../shared/hooks/usePageVisible", () => ({
  usePageVisible: () => pageVisibility.visible
}));

const client = createMockApiClient();
const get = client.get;
apiClientState.client = client;

describe("useSpaceChangeSync", () => {
  beforeEach(() => {
    pageVisibility.visible = true;
    get.mockReset();
  });

  afterEach(() => {
    cleanup();
    focusManager.setFocused(undefined);
    onlineManager.setOnline(true);
    get.mockReset();
    vi.restoreAllMocks();
  });

  it("backs off empty syncs through the idle schedule and resets on activity", () => {
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    const backoff = createSpaceChangePollingBackoff();

    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[0]);
    backoff.record(response(10));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[0]);
    backoff.record(response(10));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[1]);
    backoff.record(response(10));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[2]);
    backoff.record(response(10));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[3]);
    backoff.record(response(10));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[3]);

    backoff.record(response(11, [change(11)]));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[0]);
    backoff.record(response(11));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[1]);

    backoff.reset();
    backoff.record(response(11));
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[0]);
    backoff.record({ ...response(12), resync_required: true });
    expect(backoff.currentInterval()).toBe(POLLING.spaceChangesIdleMs[0]);
  });

  it("establishes a baseline, then applies every returned change without a Space-wide refresh", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    get
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(12, [
        change(11, { node_id: "text-1", affected_parent_ids: ["parent-1"] }),
        change(12, { node_id: "text-2", affected_parent_ids: ["parent-2"] })
      ]));
    const queryClient = createTestQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const reset = vi.spyOn(queryClient, "resetQueries");
    let renderCount = 0;

    renderHook(() => {
      renderCount += 1;
      useSpaceChangeSync("space-1");
    }, { wrapper: createWrapper(queryClient) });

    await waitForSignal(queryClient, 10);
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
    expect(invalidate).not.toHaveBeenCalled();
    const baselineRenderCount = renderCount;

    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[1]);
    expect(invalidate).not.toHaveBeenCalled();
    expect(renderCount).toBe(baselineRenderCount);

    await refetchSignal(queryClient);
    await waitForSignal(queryClient, 12);
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
    await waitFor(() => {
      expect(reset).toHaveBeenCalledWith({
        queryKey: queryKeys.children("space-1", "parent-1")
      });
      expect(reset).toHaveBeenCalledWith({
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
    const queryClient = createTestQueryClient();
    const previewKey = queryKeys.filePreviewUrl("space-1", "file-1", "pdf");
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
    const queryClient = createTestQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const reset = vi.spyOn(queryClient, "resetQueries");

    renderHook(() => useSpaceChangeSync("space-1"), { wrapper: createWrapper(queryClient) });

    await waitForSignal(queryClient, 30);
    await refetchSignal(queryClient);
    await waitForSignal(queryClient, 40);

    await waitFor(() => {
      expect(reset).toHaveBeenCalledWith({
        queryKey: queryKeys.childrenFamily("space-1")
      });
      expect(reset).toHaveBeenCalledWith({
        queryKey: queryKeys.linkStatuses("space-1")
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.linkLists("space-1"),
        refetchType: "none"
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

  it("resets a backed-off cadence after focus and reconnect", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    get.mockResolvedValue(response(10));
    const queryClient = createTestQueryClient();

    renderHook(() => useSpaceChangeSync("space-1"), {
      wrapper: createWrapper(queryClient)
    });

    await waitForSignal(queryClient, 10);
    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(3));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[2]);

    act(() => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
    });
    expect(get).toHaveBeenCalledTimes(3);
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);

    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(4));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(5));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[1]);

    act(() => {
      onlineManager.setOnline(false);
      onlineManager.setOnline(true);
    });
    expect(get).toHaveBeenCalledTimes(5);
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
  });

  it("pauses while hidden and restarts at the initial cadence when visible", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    get.mockResolvedValue(response(10));
    const queryClient = createTestQueryClient();
    const view = renderHook(
      ({ currentSpaceId }) => useSpaceChangeSync(currentSpaceId),
      {
        initialProps: { currentSpaceId: "space-1" },
        wrapper: createWrapper(queryClient)
      }
    );

    await waitForSignal(queryClient, 10);
    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[1]);

    pageVisibility.visible = false;
    view.rerender({ currentSpaceId: "space-1" });
    expect(signalInterval(queryClient, "space-1")).toBe(false);

    pageVisibility.visible = true;
    view.rerender({ currentSpaceId: "space-1" });
    expect(get).toHaveBeenCalledTimes(2);
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
  });

  it("starts a fresh cadence when the active Space changes", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    get.mockResolvedValue(response(10));
    const queryClient = createTestQueryClient();
    const view = renderHook(
      ({ currentSpaceId }) => useSpaceChangeSync(currentSpaceId),
      {
        initialProps: { currentSpaceId: "space-1" },
        wrapper: createWrapper(queryClient)
      }
    );

    await waitForSignal(queryClient, 10, "space-1");
    await refetchSignal(queryClient, "space-1");
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    await refetchSignal(queryClient, "space-1");
    await waitFor(() => expect(get).toHaveBeenCalledTimes(3));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[2]);

    view.rerender({ currentSpaceId: "space-2" });
    await waitForSignal(queryClient, 10, "space-2");

    expect(get.mock.calls[3]?.[0]).toBe(
      "/api/v1/spaces/space-2/file-change-sync?limit=100"
    );
    expect(signalInterval(queryClient, "space-2")).toBe(POLLING.spaceChangesIdleMs[0]);

    view.rerender({ currentSpaceId: "space-1" });
    expect(get).toHaveBeenCalledTimes(4);
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);

    await refetchSignal(queryClient, "space-1");
    await waitFor(() => expect(get).toHaveBeenCalledTimes(5));
    expect(get.mock.calls[4]?.[0]).toBe(
      "/api/v1/spaces/space-1/file-change-sync?limit=100&after_id=10"
    );
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
  });

  it("resets the cadence after a sync error", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    get
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(10))
      .mockRejectedValueOnce(new Error("sync unavailable"));
    const queryClient = createTestQueryClient();

    renderHook(() => useSpaceChangeSync("space-1"), {
      wrapper: createWrapper(queryClient)
    });

    await waitForSignal(queryClient, 10);
    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[1]);

    await refetchSignal(queryClient);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(3));
    expect(signalInterval(queryClient, "space-1")).toBe(POLLING.spaceChangesIdleMs[0]);
  });

  it("falls back once for a new sync error without replaying it after recovery", async () => {
    get
      .mockRejectedValueOnce(new Error("sync unavailable"))
      .mockResolvedValueOnce(response(10));
    const queryClient = createTestQueryClient();
    const refetchQueries = vi.spyOn(queryClient, "refetchQueries");

    const view = renderHook(() => useSpaceChangeSync("space-1"), {
      wrapper: createWrapper(queryClient)
    });

    await waitFor(() => expect(refetchQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.nodes("space-1"),
      type: "active"
    }));
    expect(refetchQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.texts("space-1"),
      type: "active"
    });

    pageVisibility.visible = false;
    view.rerender();
    pageVisibility.visible = true;
    view.rerender();

    await waitForSignal(queryClient, 10);
    expect(refetchQueries).toHaveBeenCalledTimes(2);
  });

  it("serializes sync requests so an older response cannot overwrite a newer token", async () => {
    const first = deferred<ReturnType<typeof response>>();
    get
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(response(11, [change(11)]));
    const queryClient = createTestQueryClient();
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

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

async function refetchSignal(queryClient: QueryClient, spaceId = "space-1") {
  await queryClient.refetchQueries({
    queryKey: queryKeys.spaceChangeSignal(spaceId),
    exact: true
  });
}

async function waitForSignal(
  queryClient: QueryClient,
  eventId: number,
  spaceId = "space-1"
) {
  await waitFor(() => {
    const signal = queryClient.getQueryData<ReturnType<typeof response>>(
      queryKeys.spaceChangeSignal(spaceId)
    );
    expect(signal?.next_after_id).toBe(eventId);
  });
}

function signalInterval(queryClient: QueryClient, spaceId: string): number | false {
  const query = queryClient.getQueryCache().find({
    queryKey: queryKeys.spaceChangeSignal(spaceId),
    exact: true
  });
  if (!query) throw new Error(`Missing Space change query for ${spaceId}`);
  const intervalOption = query.observers[0]?.options.refetchInterval;
  const interval = typeof intervalOption === "function"
    ? intervalOption(query)
    : intervalOption;
  return interval ?? false;
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
