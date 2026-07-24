import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";

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
    enabled
  });
}

export function useRecentNodesQuery(spaceId: string) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.recent(spaceId),
    queryFn: () => listNodes(client, spaceId, { sort: "updated_at_desc" })
  });
}

export function useRefreshSpace() {
  const queryClient = useQueryClient();
  return (spaceId: string) => invalidateSpaceResources(queryClient, spaceId);
}
