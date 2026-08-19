import {
  focusManager,
  onlineManager,
  QueryClientProvider,
  type InfiniteData
} from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  getSpaceLinkIndexStatus,
  listNodeLinks,
  requestNodeLinkSync,
  requestSpaceLinkReindex,
  type NodeLinksResponse
} from "../../api/links";
import { queryKeys } from "../../api/queryKeys";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  linkStatusPollInterval,
  projectionPollInterval,
  useNodeLinksQuery,
  useReindexSpaceLinksMutation,
  useSpaceLinkIndexStatusQuery,
  useSyncNodeLinksMutation
} from "./useLinkQueries";

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/links", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../api/links")>();
  return {
    ...original,
    listNodeLinks: vi.fn(),
    requestNodeLinkSync: vi.fn(),
    requestSpaceLinkReindex: vi.fn(),
    getSpaceLinkIndexStatus: vi.fn()
  };
});

describe("link queries and mutations", () => {
  beforeEach(() => {
    vi.mocked(requestNodeLinkSync).mockReset();
    vi.mocked(requestSpaceLinkReindex).mockReset();
    vi.mocked(getSpaceLinkIndexStatus).mockReset();
    vi.mocked(listNodeLinks).mockReset();
  });

  afterEach(() => {
    focusManager.setFocused(undefined);
    onlineManager.setOnline(true);
  });

  it("marks a node pending before requesting its link sync", async () => {
    const node = makeRestNode();
    const queryClient = createTestQueryClient();
    vi.mocked(requestNodeLinkSync).mockResolvedValue({ status: "accepted" });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderLinkHook(queryClient, useSyncNodeLinksMutation);

    await act(async () => {
      await result.current.mutateAsync(node);
    });

    expect(requestNodeLinkSync).toHaveBeenCalledWith(expect.anything(), node.space_id, node.id);
    expect(queryClient.getQueryData(queryKeys.nodeLinkStatus(node.space_id, node.id))).toMatchObject({
      status: "pending"
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.nodeLinkStatus(node.space_id, node.id),
      exact: true
    });
  });

  it("resets the Space link family after accepting a full reindex", async () => {
    const queryClient = createTestQueryClient();
    vi.mocked(requestSpaceLinkReindex).mockResolvedValue({ status: "accepted" });
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderLinkHook(queryClient, useReindexSpaceLinksMutation);

    act(() => result.current.mutate("space-1"));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkStatuses("space-1")
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkLists("space-1"),
      refetchType: "none"
    });
    expect(queryClient.getQueryData(queryKeys.spaceLinkIndexStatus("space-1"))).toEqual({
      pending: true
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.spaceLinkIndexStatus("space-1"),
      exact: true
    });
  });

  it("loads the authoritative Space link index status", async () => {
    const queryClient = createTestQueryClient();
    vi.mocked(getSpaceLinkIndexStatus).mockResolvedValue({ pending: true });
    const { result } = renderLinkHook(
      queryClient,
      () => useSpaceLinkIndexStatusQuery("space-1", true)
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(getSpaceLinkIndexStatus).toHaveBeenCalledWith(expect.anything(), "space-1");
    expect(result.current.data).toEqual({ pending: true });
  });

  it("passes the server cursor through paginated link requests", async () => {
    const node = makeRestNode();
    const queryClient = createTestQueryClient();
    vi.mocked(listNodeLinks)
      .mockResolvedValueOnce(linkPage("source-1", true, "next-cursor"))
      .mockResolvedValueOnce(linkPage("source-2", false, null));
    const { result } = renderLinkHook(
      queryClient,
      () => useNodeLinksQuery(node, "incoming", true)
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await act(async () => {
      await result.current.fetchNextPage();
    });

    expect(listNodeLinks).toHaveBeenNthCalledWith(
      1,
      expect.anything(),
      node.space_id,
      node.id,
      "incoming",
      null
    );
    expect(listNodeLinks).toHaveBeenNthCalledWith(
      2,
      expect.anything(),
      node.space_id,
      node.id,
      "incoming",
      "next-cursor"
    );
  });

  it("does not replay cached invalidated link pages on mount", async () => {
    const node = makeRestNode();
    const queryClient = createTestQueryClient();
    const queryKey = queryKeys.nodeLinkList(node.space_id, node.id, "incoming");
    queryClient.setQueryData<InfiniteData<NodeLinksResponse, string | null>>(queryKey, {
      pages: [
        linkPage("source-1", true, "next-cursor"),
        linkPage("source-2", false, null)
      ],
      pageParams: [null, "next-cursor"]
    });
    await queryClient.invalidateQueries({
      queryKey,
      exact: true,
      refetchType: "none"
    });

    const { result } = renderLinkHook(
      queryClient,
      () => useNodeLinksQuery(node, "incoming", true)
    );

    expect(result.current.data?.pages).toHaveLength(2);
    expect(result.current.isFetching).toBe(false);
    expect(listNodeLinks).not.toHaveBeenCalled();
  });

  it("does not replay cached invalidated link pages on focus or reconnect", async () => {
    const node = makeRestNode();
    const queryClient = createTestQueryClient({
      defaultOptions: {
        queries: {
          refetchOnWindowFocus: true,
          refetchOnReconnect: true
        }
      }
    });
    const queryKey = queryKeys.nodeLinkList(node.space_id, node.id, "incoming");
    queryClient.setQueryData<InfiniteData<NodeLinksResponse, string | null>>(queryKey, {
      pages: [linkPage("source-1", false, null)],
      pageParams: [null]
    });
    await queryClient.invalidateQueries({
      queryKey,
      exact: true,
      refetchType: "none"
    });
    const view = renderLinkHook(
      queryClient,
      () => useNodeLinksQuery(node, "incoming", true)
    );

    await act(async () => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
      onlineManager.setOnline(false);
      onlineManager.setOnline(true);
    });

    expect(view.result.current.data?.pages).toHaveLength(1);
    expect(listNodeLinks).not.toHaveBeenCalled();
    view.unmount();
  });

  it("refetches cached stale link data when it was not explicitly invalidated", async () => {
    const node = makeRestNode();
    const queryClient = createTestQueryClient();
    const queryKey = queryKeys.nodeLinkList(node.space_id, node.id, "incoming");
    queryClient.setQueryData<InfiniteData<NodeLinksResponse, string | null>>(queryKey, {
      pages: [linkPage("source-1", false, null)],
      pageParams: [null]
    });
    vi.mocked(listNodeLinks).mockResolvedValue(linkPage("source-2", false, null));

    renderLinkHook(
      queryClient,
      () => useNodeLinksQuery(node, "incoming", true)
    );

    await waitFor(() => expect(listNodeLinks).toHaveBeenCalledTimes(1));
  });
});

describe("projection polling", () => {
  it("polls syncing work faster and stops after a terminal status", () => {
    expect(projectionPollInterval(linkStatus("pending"))).toBe(15_000);
    expect(projectionPollInterval(linkStatus("syncing"))).toBe(3_000);
    expect(projectionPollInterval(linkStatus("idle", true))).toBe(15_000);
    expect(projectionPollInterval(linkStatus("failed", true))).toBe(15_000);
    expect(projectionPollInterval(linkStatus("idle"))).toBe(false);
    expect(projectionPollInterval(linkStatus("failed"))).toBe(false);
    expect(projectionPollInterval(undefined)).toBe(false);
  });

  it("retries a failed status query only while link data is invalidated", () => {
    expect(linkStatusPollInterval(linkStatus("syncing"), true, true)).toBe(3_000);
    expect(linkStatusPollInterval(undefined, true, true)).toBe(15_000);
    expect(linkStatusPollInterval(undefined, true, false)).toBe(false);
    expect(linkStatusPollInterval(undefined, false, true)).toBe(false);
  });
});

function renderLinkHook<Result>(queryClient: ReturnType<typeof createTestQueryClient>, hook: () => Result) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(hook, { wrapper });
}

function linkPage(nodeId: string, hasMore: boolean, nextCursor: string | null) {
  return {
    links: [{
      node_id: nodeId,
      path: `/${nodeId}.md`,
      kind: "link" as const,
      occurrence_count: 1
    }],
    page: {
      limit: 50,
      returned: 1,
      has_more: hasMore,
      next_cursor: nextCursor
    }
  };
}

function linkStatus(status: "idle" | "pending" | "syncing" | "failed", spacePending = false) {
  return {
    status,
    space_pending: spacePending,
    projected_at: null,
    failure_code: status === "failed" ? "job_failed" : null,
    failed_at: status === "failed" ? "2026-08-19T00:00:00Z" : null
  };
}
