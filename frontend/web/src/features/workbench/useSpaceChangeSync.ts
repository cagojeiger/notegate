import { useEffect, useMemo, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listFileChangeEvents } from "../../api/events";
import { POLLING, withPollingJitter } from "../../api/polling";
import { invalidateSpaceResources } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { usePageVisible } from "../../shared/hooks/usePageVisible";

export function useSpaceChangeSync(spaceId: string | null) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const pageVisible = usePageVisible();
  const lastSeenBySpace = useRef(new Map<string, number | null>());
  const refetchInterval = useMemo(
    () => withPollingJitter(POLLING.spaceChangesMs, POLLING.spaceChangesJitterMs),
    []
  );
  const query = useQuery({
    queryKey: spaceId ? queryKeys.spaceChangeSignal(spaceId) : ["sync", "space-change", "none"],
    queryFn: () => {
      if (!spaceId) throw new Error("No active space");
      return listFileChangeEvents(client, spaceId, { limit: 1 });
    },
    select: (data) => data.events[0] ?? null,
    enabled: Boolean(spaceId) && pageVisible,
    refetchInterval: pageVisible ? refetchInterval : false,
    staleTime: refetchInterval,
    notifyOnChangeProps: ["data"]
  });
  const latestEventId = query.data?.id ?? null;

  useEffect(() => {
    if (!spaceId || query.data === undefined) return;

    if (!lastSeenBySpace.current.has(spaceId)) {
      lastSeenBySpace.current.set(spaceId, latestEventId);
      return;
    }
    if (lastSeenBySpace.current.get(spaceId) === latestEventId) return;

    lastSeenBySpace.current.set(spaceId, latestEventId);
    invalidateSpaceResources(queryClient, spaceId);

    if (query.data?.op_type === "item.delete") {
      const previewKey = query.data.metadata.item_kind === "folder"
        ? queryKeys.filePreviewUrls(spaceId)
        : query.data.node_id
          ? queryKeys.filePreviewUrl(spaceId, query.data.node_id)
          : queryKeys.filePreviewUrls(spaceId);
      queryClient.removeQueries({ queryKey: previewKey });
    }
  }, [latestEventId, query.data, queryClient, spaceId]);
}
