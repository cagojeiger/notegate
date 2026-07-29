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
import type { CurrentUserUsage } from "../../api/usage";
import { createMockApiClient } from "../../test/apiClient";
import { createTestQueryClient } from "../../test/queryClient";
import {
  createUsagePollingBackoff,
  useCheckSpaceUsageMutation,
  useUsageQuery
} from "./useUsageQueries";

const apiClientState = vi.hoisted(
  (): { client: ApiClient | null } => ({ client: null })
);

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => apiClientState.client!
}));

const client = createMockApiClient();
const get = client.get;
const post = client.post;
apiClientState.client = client;

describe("createUsagePollingBackoff", () => {
  it("backs off unchanged summaries, caps at five minutes, and resets on activity", () => {
    const backoff = createUsagePollingBackoff();
    const stableUsage = usage();

    expect(backoff.currentInterval()).toBe(POLLING.usageSummaryIdleMs[0]);
    backoff.record(stableUsage);
    expect(backoff.currentInterval(stableUsage)).toBe(POLLING.usageSummaryIdleMs[0]);
    backoff.record(stableUsage);
    expect(backoff.currentInterval(stableUsage)).toBe(POLLING.usageSummaryIdleMs[1]);
    backoff.record(stableUsage);
    expect(backoff.currentInterval(stableUsage)).toBe(POLLING.usageSummaryIdleMs[2]);
    backoff.record(stableUsage);
    expect(backoff.currentInterval(stableUsage)).toBe(POLLING.usageSummaryIdleMs[2]);

    const changedUsage = usage({ itemsUsed: 320 });
    backoff.record(changedUsage);
    expect(backoff.currentInterval(changedUsage)).toBe(POLLING.usageSummaryIdleMs[0]);

    const pendingUsage = usage({ pending: true, itemsUsed: 320 });
    backoff.record(pendingUsage);
    expect(backoff.currentInterval(pendingUsage)).toBe(POLLING.usagePendingMs);
    backoff.record(changedUsage);
    expect(backoff.currentInterval(changedUsage)).toBe(POLLING.usageSummaryIdleMs[0]);

    backoff.record(changedUsage);
    expect(backoff.currentInterval(changedUsage)).toBe(POLLING.usageSummaryIdleMs[1]);
    backoff.reset();
    expect(backoff.currentInterval(changedUsage)).toBe(POLLING.usageSummaryIdleMs[0]);
  });

  it("uses the pending cadence when any Space is reconciling", () => {
    const backoff = createUsagePollingBackoff();
    const baseUsage = usage();
    const mixedUsage: CurrentUserUsage = {
      ...baseUsage,
      spaces: [
        baseUsage.spaces[0],
        { ...baseUsage.spaces[0], id: "space-2", reconciliation_pending: true }
      ]
    };

    expect(backoff.currentInterval(mixedUsage)).toBe(POLLING.usagePendingMs);
  });
});

describe("useUsageQuery", () => {
  beforeEach(() => {
    get.mockReset();
    post.mockReset();
  });

  afterEach(() => {
    cleanup();
    focusManager.setFocused(undefined);
    onlineManager.setOnline(true);
    vi.restoreAllMocks();
  });

  it("applies the adaptive interval to the active query observer", async () => {
    get.mockResolvedValue(usage());
    const queryClient = testQueryClient();

    renderHook(() => useUsageQuery(), { wrapper: createWrapper(queryClient) });

    await waitFor(() => expect(get).toHaveBeenCalledTimes(1));
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[0]);

    await refetchUsage(queryClient);
    expect(get).toHaveBeenCalledTimes(2);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[1]);

    await refetchUsage(queryClient);
    expect(get).toHaveBeenCalledTimes(3);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[2]);

    await refetchUsage(queryClient);
    expect(get).toHaveBeenCalledTimes(4);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[2]);
  });

  it("resets a backed-off observer after focus, reconnect, and errors", async () => {
    get.mockResolvedValue(usage());
    const queryClient = testQueryClient();

    renderHook(() => useUsageQuery(), { wrapper: createWrapper(queryClient) });
    await waitFor(() => expect(get).toHaveBeenCalledTimes(1));
    await refetchUsage(queryClient);
    await refetchUsage(queryClient);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[2]);

    act(() => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
    });
    expect(get).toHaveBeenCalledTimes(3);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[0]);

    await refetchUsage(queryClient);
    await refetchUsage(queryClient);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[1]);

    act(() => {
      onlineManager.setOnline(false);
      onlineManager.setOnline(true);
    });
    expect(get).toHaveBeenCalledTimes(5);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[0]);

    await refetchUsage(queryClient);
    get.mockRejectedValueOnce(new Error("usage unavailable"));
    await refetchUsage(queryClient);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[0]);
  });

  it("switches a backed-off owner to pending polling after a manual check", async () => {
    get.mockResolvedValue(usage());
    post.mockResolvedValue({ status: "queued" });
    const queryClient = testQueryClient();
    const view = renderHook(() => ({
      usage: useUsageQuery(),
      check: useCheckSpaceUsageMutation()
    }), { wrapper: createWrapper(queryClient) });

    await waitFor(() => expect(get).toHaveBeenCalledTimes(1));
    await refetchUsage(queryClient);
    await refetchUsage(queryClient);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[2]);

    get.mockResolvedValue(usage({ pending: true }));
    await act(async () => {
      await view.result.current.check.mutateAsync("space-1");
    });
    await waitFor(() => {
      expect(queryClient.getQueryData<CurrentUserUsage>(queryKeys.usage))
        .toEqual(usage({ pending: true }));
    });
    expect(post).toHaveBeenCalledWith("/api/v1/spaces/space-1/usage/reconcile");
    expect(usageInterval(queryClient)).toBe(POLLING.usagePendingMs);

    get.mockResolvedValue(usage());
    await refetchUsage(queryClient);
    expect(usageInterval(queryClient)).toBe(POLLING.usageSummaryIdleMs[0]);
  });

  it("can disable usage loading for non-user callers", () => {
    const queryClient = testQueryClient();

    renderHook(() => useUsageQuery(false), { wrapper: createWrapper(queryClient) });

    expect(get).not.toHaveBeenCalled();
  });
});

function testQueryClient() {
  return createTestQueryClient({
    defaultOptions: {
      queries: {
        staleTime: 5_000
      }
    }
  });
}

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

async function refetchUsage(queryClient: QueryClient) {
  await queryClient.refetchQueries({
    queryKey: queryKeys.usage,
    exact: true
  });
}

function usageInterval(queryClient: QueryClient): number | false {
  const query = queryClient.getQueryCache().find({
    queryKey: queryKeys.usage,
    exact: true
  });
  if (!query) throw new Error("Missing usage query");
  const intervalOption = query.observers[0]?.options.refetchInterval;
  const interval = typeof intervalOption === "function"
    ? intervalOption(query)
    : intervalOption;
  return interval ?? false;
}

function usage({
  pending = false,
  itemsUsed = 319
}: {
  pending?: boolean;
  itemsUsed?: number;
} = {}): CurrentUserUsage {
  return {
    tier: "tier0",
    spaces: [{
      id: "space-1",
      name: "Personal",
      items: { used: itemsUsed, limit: 1_999 },
      text_bytes: { used: 48_120_320, limit: 134_217_728 },
      file_bytes: { used: 80_000_000, limit: 134_217_728 },
      reconciliation_pending: pending
    }]
  };
}
