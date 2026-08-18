import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listNodeLinks, requestNodeLinkSync, requestSpaceLinkReindex } from "../../api/links";
import { queryKeys } from "../../api/queryKeys";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  projectionPollInterval,
  useNodeLinksQuery,
  useReindexSpaceLinksMutation,
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
    requestSpaceLinkReindex: vi.fn()
  };
});

describe("link queries and mutations", () => {
  beforeEach(() => {
    vi.mocked(requestNodeLinkSync).mockReset();
    vi.mocked(requestSpaceLinkReindex).mockReset();
    vi.mocked(listNodeLinks).mockReset();
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
    expect(result.current.data?.pages).toHaveLength(2);
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
