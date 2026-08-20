import { useCallback, useEffect, useReducer } from "react";
import {
  focusManager,
  onlineManager,
  useMutation,
  useQuery,
  useQueryClient
} from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import { getCurrentUserUsage, requestSpaceUsageCheck, type CurrentUserUsage } from "../../api/usage";

export function createUsagePollingBackoff() {
  let idleStep = 0;
  let previousSnapshot: string | null = null;

  return {
    currentInterval(usage?: CurrentUserUsage) {
      if (hasPendingReconciliation(usage)) return POLLING.usagePendingMs;
      return POLLING.usageSummaryIdleMs[idleStep]
        ?? POLLING.usageSummaryIdleMs[0];
    },
    record(usage: CurrentUserUsage) {
      if (hasPendingReconciliation(usage)) {
        previousSnapshot = null;
        idleStep = 0;
        return;
      }

      const snapshot = JSON.stringify(usage);
      if (previousSnapshot === null || snapshot !== previousSnapshot) {
        previousSnapshot = snapshot;
        idleStep = 0;
        return;
      }
      idleStep = Math.min(idleStep + 1, POLLING.usageSummaryIdleMs.length - 1);
    },
    reset() {
      previousSnapshot = null;
      idleStep = 0;
    }
  };
}

type UsagePollingBackoff = ReturnType<typeof createUsagePollingBackoff>;

const pollingBackoffs = new WeakMap<QueryClient, UsagePollingBackoff>();

function pollingBackoffFor(queryClient: QueryClient) {
  const existing = pollingBackoffs.get(queryClient);
  if (existing) return existing;
  const backoff = createUsagePollingBackoff();
  pollingBackoffs.set(queryClient, backoff);
  return backoff;
}

export function useUsageQuery(enabled = true) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const pollingBackoff = pollingBackoffFor(queryClient);
  const [, reschedulePolling] = useReducer((revision: number) => revision + 1, 0);
  const resetPolling = useCallback(() => {
    pollingBackoff.reset();
    reschedulePolling();
  }, [pollingBackoff]);

  useEffect(() => {
    if (!enabled) return;
    resetPolling();
    const unsubscribeFocus = focusManager.subscribe((focused) => {
      if (focused) resetPolling();
    });
    const unsubscribeOnline = onlineManager.subscribe((online) => {
      if (online) resetPolling();
    });
    return () => {
      unsubscribeFocus();
      unsubscribeOnline();
    };
  }, [enabled, resetPolling]);

  return useQuery({
    queryKey: queryKeys.usage,
    queryFn: async () => {
      try {
        const usage = await getCurrentUserUsage(client);
        pollingBackoff.record(usage);
        return usage;
      } catch (error) {
        pollingBackoff.reset();
        throw error;
      }
    },
    enabled,
    refetchInterval: (query) => pollingBackoff.currentInterval(query.state.data)
  });
}

export function useCheckSpaceUsageMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const pollingBackoff = pollingBackoffFor(queryClient);
  return useMutation({
    mutationFn: (spaceId: string) => requestSpaceUsageCheck(client, spaceId),
    meta: { silentError: true },
    onSettled: (response, error, spaceId) => {
      const pending = response?.availability.reason === "pending";
      const cooldown = error instanceof ApiError && error.kind === "usage_reconciliation_cooldown";
      if (!pending && !cooldown) return;

      pollingBackoff.reset();
      if (pending && response) {
        queryClient.setQueryData<CurrentUserUsage>(queryKeys.usage, (current) => current ? {
          ...current,
          spaces: current.spaces.map((space) => space.id === spaceId
            ? {
              ...space,
              reconciliation: {
                status: "pending",
                availability: response.availability
              }
            }
            : space)
        } : current);
      }
      void queryClient.invalidateQueries({ queryKey: queryKeys.usage });
    }
  });
}

function hasPendingReconciliation(usage?: CurrentUserUsage) {
  return usage?.spaces.some((space) => space.reconciliation?.status === "pending") === true;
}
