import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren, listNodes } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";

export function useNodeChildrenQuery(spaceId: string, nodeId: string, enabled: boolean) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.children(spaceId, nodeId),
    queryFn: ({ pageParam }) => listChildren(client, spaceId, nodeId, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (lastPage.page.has_more ? lastPage.page.next_cursor : undefined),
    // Poll while the tab is visible so externally-created nodes (MCP/REST) appear.
    refetchInterval: 20_000,
    enabled
  });
}

export function useRecentNodesQuery(spaceId: string) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.recent(spaceId), queryFn: () => listNodes(client, spaceId, { sort: "updated_at_desc" }), refetchInterval: 15_000 });
}

export function useRefreshSpace() {
  const queryClient = useQueryClient();
  return (spaceId: string) => void queryClient.invalidateQueries({ queryKey: ["spaces", spaceId] });
}
