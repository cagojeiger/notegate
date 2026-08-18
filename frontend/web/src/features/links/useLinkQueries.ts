import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import {
  getNodeLinkStatus,
  listNodeLinks,
  requestNodeLinkSync,
  requestSpaceLinkReindex,
  type NodeLinkDirection,
  type NodeLinkProjectionStatus
} from "../../api/links";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";

const PENDING_STATUS_POLL_MS = 15_000;
const SYNCING_STATUS_POLL_MS = 3_000;

export function useNodeLinkStatusQuery(node: RestNode, enabled: boolean) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.nodeLinkStatus(node.space_id, node.id),
    queryFn: () => getNodeLinkStatus(client, node.space_id, node.id),
    enabled,
    refetchInterval: (query) => projectionPollInterval(query.state.data)
  });
}

export function useNodeLinksQuery(
  node: RestNode,
  direction: NodeLinkDirection,
  enabled: boolean
) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: queryKeys.nodeLinkList(node.space_id, node.id, direction),
    queryFn: ({ pageParam }) => listNodeLinks(
      client,
      node.space_id,
      node.id,
      direction,
      pageParam
    ),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (
      lastPage.page.has_more ? lastPage.page.next_cursor : undefined
    ),
    enabled
  });
}

export function useSyncNodeLinksMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: (node: RestNode) => requestNodeLinkSync(client, node.space_id, node.id),
    onMutate: async (node) => {
      await queryClient.cancelQueries({
        queryKey: queryKeys.nodeLinkStatus(node.space_id, node.id),
        exact: true
      });
      queryClient.setQueryData<NodeLinkProjectionStatus>(
        queryKeys.nodeLinkStatus(node.space_id, node.id),
        (current) => ({
          status: "pending",
          projected_at: current?.projected_at ?? null,
          failure_code: null,
          failed_at: null
        })
      );
    },
    onSettled: (_response, _error, node) => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.nodeLinkStatus(node.space_id, node.id),
        exact: true
      });
    }
  });
}

export function useReindexSpaceLinksMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: (spaceId: string) => requestSpaceLinkReindex(client, spaceId),
    onSuccess: (_response, spaceId) => {
      void queryClient.resetQueries({ queryKey: queryKeys.links(spaceId) });
    }
  });
}

function projectionPollInterval(status: NodeLinkProjectionStatus | undefined): number | false {
  if (status?.status === "pending") return PENDING_STATUS_POLL_MS;
  if (status?.status === "syncing") return SYNCING_STATUS_POLL_MS;
  return false;
}
