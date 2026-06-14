import { useMemo } from "react";
import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren, listNodes } from "../../api/nodes";
import { POLLING, withPollingJitter } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import { usePageVisible } from "../../shared/hooks/usePageVisible";

export function useNodeChildrenQuery(spaceId: string, nodeId: string, enabled: boolean) {
  const client = useApiClient();
  const pageVisible = usePageVisible();
  const refetchInterval = useMemo(() => withPollingJitter(POLLING.treeChildrenMs, POLLING.treeChildrenJitterMs), []);
  return useInfiniteQuery({
    queryKey: queryKeys.children(spaceId, nodeId),
    queryFn: ({ pageParam }) => listChildren(client, spaceId, nodeId, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    refetchInterval: pageVisible ? refetchInterval : false,
    enabled: enabled && pageVisible
  });
}

export function useRecentNodesQuery(spaceId: string) {
  const client = useApiClient();
  const pageVisible = usePageVisible();
  const refetchInterval = useMemo(() => withPollingJitter(POLLING.recentMs, POLLING.recentJitterMs), []);
  return useQuery({
    queryKey: queryKeys.recent(spaceId),
    queryFn: () => listNodes(client, spaceId, { sort: "updated_at_desc" }),
    enabled: pageVisible,
    refetchInterval: pageVisible ? refetchInterval : false
  });
}

export function useRefreshSpace() {
  const queryClient = useQueryClient();
  return (spaceId: string) => void queryClient.invalidateQueries({ queryKey: ["spaces", spaceId] });
}
