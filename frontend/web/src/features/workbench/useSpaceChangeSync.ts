import { useCallback, useEffect, useMemo, useReducer, useRef } from "react";
import {
  focusManager,
  onlineManager,
  useQuery,
  useQueryClient
} from "@tanstack/react-query";
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

export function createSpaceChangePollingBackoff() {
  let baselineEstablished = false;
  let idleStep = 0;
  let scheduledInterval = nextInterval();

  function nextInterval() {
    const baseInterval = POLLING.spaceChangesIdleMs[idleStep]
      ?? POLLING.spaceChangesIdleMs[0];
    return withPollingJitter(baseInterval, POLLING.spaceChangesJitterMs);
  }

  function setIdleStep(nextStep: number) {
    idleStep = Math.min(nextStep, POLLING.spaceChangesIdleMs.length - 1);
    scheduledInterval = nextInterval();
  }

  return {
    currentInterval() {
      return scheduledInterval;
    },
    record(response: Pick<FileChangeSyncResponse, "changes" | "resync_required">) {
      if (response.resync_required || response.changes.length > 0) {
        baselineEstablished = true;
        setIdleStep(0);
        return;
      }
      if (!baselineEstablished) {
        baselineEstablished = true;
        setIdleStep(0);
        return;
      }
      setIdleStep(idleStep + 1);
    },
    reset() {
      baselineEstablished = false;
      setIdleStep(0);
    }
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
  const pollingBackoff = useMemo(createSpaceChangePollingBackoff, [spaceId]);
  const handledSyncErrors = useRef(new Map<string, number>());
  const [, reschedulePolling] = useReducer((revision: number) => revision + 1, 0);
  const resetPolling = useCallback(() => {
    pollingBackoff.reset();
    reschedulePolling();
  }, [pollingBackoff]);

  useEffect(() => {
    if (pageVisible) resetPolling();
  }, [pageVisible, resetPolling]);

  useEffect(() => {
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
  }, [resetPolling]);

  const syncQuery = useQuery({
    queryKey: spaceId ? queryKeys.spaceChangeSignal(spaceId) : ["sync", "space-change", "none"],
    queryFn: async () => {
      if (!spaceId) throw new Error("No active space");
      try {
        const response = await syncSpaceChanges(spaceId);
        pollingBackoff.record(response);
        return response;
      } catch (error) {
        pollingBackoff.reset();
        throw error;
      }
    },
    enabled: Boolean(spaceId) && pageVisible,
    refetchInterval: pageVisible
      ? () => pollingBackoff.currentInterval()
      : false,
    // Opened node and text queries delegate external freshness to this signal.
    // Match their former focus threshold without attaching one refetch per editor.
    staleTime: POLLING.spaceChangesFocusFreshMs,
    notifyOnChangeProps: ["data", "errorUpdatedAt"]
  });

  useEffect(() => {
    if (!spaceId || syncQuery.errorUpdatedAt === 0) return;
    if (handledSyncErrors.current.get(spaceId) === syncQuery.errorUpdatedAt) return;
    handledSyncErrors.current.set(spaceId, syncQuery.errorUpdatedAt);
    if (!pageVisible) return;
    // Preserve document freshness when the centralized sync endpoint is down.
    // This intentionally spends per-document requests only in degraded mode.
    void queryClient.refetchQueries({ queryKey: queryKeys.nodes(spaceId), type: "active" });
    void queryClient.refetchQueries({ queryKey: queryKeys.texts(spaceId), type: "active" });
  }, [pageVisible, queryClient, spaceId, syncQuery.errorUpdatedAt]);
}
