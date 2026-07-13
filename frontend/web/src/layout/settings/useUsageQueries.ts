import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import { getCurrentUserUsage, requestSpaceUsageCheck, type CurrentUserUsage } from "../../api/usage";

export function useUsageQuery() {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.usage,
    queryFn: () => getCurrentUserUsage(client),
    refetchInterval: (query) => query.state.data?.spaces.some((space) => space.reconciliation_pending)
      ? POLLING.usagePendingMs
      : false
  });
}

export function useCheckSpaceUsageMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (spaceId: string) => requestSpaceUsageCheck(client, spaceId),
    meta: { silentError: true },
    onSettled: (_response, error, spaceId) => {
      const refreshPendingState = !error || (error instanceof ApiError && error.kind === "usage_reconciliation_pending");
      if (!refreshPendingState) return;

      queryClient.setQueryData<CurrentUserUsage>(queryKeys.usage, (current) => current ? {
        ...current,
        spaces: current.spaces.map((space) => space.id === spaceId
          ? { ...space, reconciliation_pending: true }
          : space)
      } : current);
      void queryClient.invalidateQueries({ queryKey: queryKeys.usage });
    }
  });
}
