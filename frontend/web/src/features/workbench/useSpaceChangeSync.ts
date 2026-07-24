import { useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import type { ApiClient } from "../../api/client";
import { drainFileChanges } from "../../api/events";
import { POLLING, withPollingJitter } from "../../api/polling";
import {
  applyExternalFileChanges,
  invalidateFileSyncFallback
} from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { FileChangeSyncResponse } from "../../api/types";
import { usePageVisible } from "../../shared/hooks/usePageVisible";

export function createSpaceChangeSynchronizer(
  client: ApiClient,
  queryClient: QueryClient
) {
  const lastAppliedBySpace = new Map<string, number>();
  const pendingBySpace = new Map<string, Promise<FileChangeSyncResponse>>();

  return function syncSpaceChanges(spaceId: string) {
    const previous = pendingBySpace.get(spaceId);
    const current = (previous ?? Promise.resolve())
      .catch(() => undefined)
      .then(async () => {
        const response = await drainFileChanges(
          client,
          spaceId,
          lastAppliedBySpace.get(spaceId)
        );
        if (response.resync_required) {
          await invalidateFileSyncFallback(queryClient, spaceId);
        } else {
          await applyExternalFileChanges(queryClient, spaceId, response.changes);
        }
        lastAppliedBySpace.set(spaceId, response.next_after_id);
        return response;
      });

    pendingBySpace.set(spaceId, current);
    void current.then(
      () => {
        if (pendingBySpace.get(spaceId) === current) pendingBySpace.delete(spaceId);
      },
      () => {
        if (pendingBySpace.get(spaceId) === current) pendingBySpace.delete(spaceId);
      }
    );
    return current;
  };
}

export function useSpaceChangeSync(spaceId: string | null) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const pageVisible = usePageVisible();
  const syncSpaceChanges = useMemo(
    () => createSpaceChangeSynchronizer(client, queryClient),
    [client, queryClient]
  );
  const refetchInterval = useMemo(
    () => withPollingJitter(POLLING.spaceChangesMs, POLLING.spaceChangesJitterMs),
    []
  );
  useQuery({
    queryKey: spaceId ? queryKeys.spaceChangeSignal(spaceId) : ["sync", "space-change", "none"],
    queryFn: () => {
      if (!spaceId) throw new Error("No active space");
      return syncSpaceChanges(spaceId);
    },
    enabled: Boolean(spaceId) && pageVisible,
    refetchInterval: pageVisible ? refetchInterval : false,
    staleTime: refetchInterval,
    notifyOnChangeProps: ["data"]
  });
}
