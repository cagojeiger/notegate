import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "../../api/client";
import { queryKeys } from "../../api/queryKeys";
import { useSpaceChangeSync } from "./useSpaceChangeSync";

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

  it("establishes a baseline without refetching resources, then invalidates once per new event", async () => {
    get
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(10))
      .mockResolvedValueOnce(response(11));
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
    await waitForSignal(queryClient, 11);
    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledOnce();
      expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.space("space-1") });
    });
  });

  it("drops stale preview URLs when an external delete event is observed", async () => {
    get
      .mockResolvedValueOnce(response(20))
      .mockResolvedValueOnce(response(21, {
        op_type: "item.delete",
        node_id: "file-1",
        metadata: { item_kind: "file" }
      }));
    const queryClient = createQueryClient();
    const previewKey = queryKeys.filePreviewUrl("space-1", "file-1");
    queryClient.setQueryData(previewKey, { url: "https://storage.example/stale" });

    renderHook(() => useSpaceChangeSync("space-1"), { wrapper: createWrapper(queryClient) });

    await waitForSignal(queryClient, 20);
    await refetchSignal(queryClient);
    await waitForSignal(queryClient, 21);

    await waitFor(() => expect(queryClient.getQueryData(previewKey)).toBeUndefined());
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
    expect(signal?.events[0]?.id).toBe(eventId);
  });
}

function response(
  id: number,
  overrides: Partial<{
    op_type: string;
    node_id: string | null;
    metadata: Record<string, unknown>;
  }> = {}
) {
  return {
    events: [{
      id,
      created_at: "2026-07-24T00:00:00Z",
      space_id: "space-1",
      node_id: "node-1",
      actor_account_id: "account-1",
      op_type: "text.write",
      metadata: {},
      ...overrides
    }],
    page: {
      limit: 1,
      returned: 1,
      has_more: false,
      next_cursor: null
    }
  };
}
