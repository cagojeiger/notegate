import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren, listNodes } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { invalidateSpaceResources } from "../../api/queryInvalidation";

export function useNodeChildrenQuery(spaceId: string, nodeId: string, enabled: boolean) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.children(spaceId, nodeId),
    queryFn: ({ pageParam }) => listChildren(client, spaceId, nodeId, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    enabled,
    // Forward file-change sync and local mutations explicitly invalidate this
    // key, so cached folders do not need time-based refetch fan-out.
    staleTime: Number.POSITIVE_INFINITY
  });
}

export function useRecentNodesQuery(spaceId: string) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.recent(spaceId),
    queryFn: ({ pageParam }) =>
      listNodes(client, spaceId, {
        sort: "updated_at_desc",
        cursor: pageParam
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) =>
      lastPage.page.has_more ? lastPage.page.next_cursor : undefined,
    staleTime: Number.POSITIVE_INFINITY
  });
}

export function useRefreshSpace() {
  const queryClient = useQueryClient();
  return (spaceId: string) => invalidateSpaceResources(queryClient, spaceId);
}
