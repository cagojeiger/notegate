import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { requestNodeLinkSync, requestSpaceLinkReindex } from "../../api/links";
import { queryKeys } from "../../api/queryKeys";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
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
    requestNodeLinkSync: vi.fn(),
    requestSpaceLinkReindex: vi.fn()
  };
});

describe("link mutations", () => {
  beforeEach(() => {
    vi.mocked(requestNodeLinkSync).mockReset();
    vi.mocked(requestSpaceLinkReindex).mockReset();
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
    const { result } = renderLinkHook(queryClient, useReindexSpaceLinksMutation);

    act(() => result.current.mutate("space-1"));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.links("space-1")
    });
  });
});

function renderLinkHook<Result>(queryClient: ReturnType<typeof createTestQueryClient>, hook: () => Result) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(hook, { wrapper });
}
