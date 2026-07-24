import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { POLLING } from "../../api/polling";
import type { CurrentUserUsage } from "../../api/usage";
import { useUsageQuery } from "./useUsageQueries";

const useQuery = vi.hoisted(() => vi.fn());

vi.mock("@tanstack/react-query", () => ({
  useQuery,
  useMutation: vi.fn(),
  useQueryClient: vi.fn()
}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

describe("useUsageQuery", () => {
  beforeEach(() => {
    useQuery.mockReset();
    useQuery.mockReturnValue({});
  });

  it("polls only while a reconciliation is pending", () => {
    renderHook(() => useUsageQuery());
    const options = useQuery.mock.calls[0][0];
    const interval = options.refetchInterval as (query: { state: { data?: CurrentUserUsage } }) => number | false;
    const usage = (reconciliationPending: boolean): CurrentUserUsage => ({
      tier: "tier0",
      spaces: [{
        id: "space-1",
        name: "Personal",
        items: { used: 0, limit: 1_999 },
        text_bytes: { used: 0, limit: 134_217_728 },
        file_bytes: { used: 0, limit: 134_217_728 },
        reconciliation_pending: reconciliationPending
      }]
    });

    expect(interval({ state: {} })).toBe(false);
    expect(interval({ state: { data: usage(false) } })).toBe(false);
    expect(interval({ state: { data: usage(true) } })).toBe(POLLING.usagePendingMs);
  });
});
