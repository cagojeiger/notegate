import { useInfiniteQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listAuditEvents, listFileChangeEvents } from "../../api/events";
import { queryKeys } from "../../api/queryKeys";

export function useAuditEventsQuery(enabled: boolean) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.auditEvents,
    queryFn: ({ pageParam }) => listAuditEvents(client, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    enabled
  });
}

export function useFileChangeEventsQuery(spaceId: string | null, nodeId: string | null, enabled: boolean) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: spaceId ? queryKeys.fileChangeEvents(spaceId, nodeId) : queryKeys.fileChangeEvents("none", nodeId),
    queryFn: ({ pageParam }) => {
      if (!spaceId) throw new Error("Space is required");
      return listFileChangeEvents(client, spaceId, { nodeId, cursor: pageParam });
    },
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    enabled: enabled && Boolean(spaceId)
  });
}
