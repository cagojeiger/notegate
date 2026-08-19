import {
  type QueryClient,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient
} from "@tanstack/react-query";

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
import { invalidateSpaceLinks } from "../../api/queryInvalidation";
import type { RestNode } from "../../api/types";

const PENDING_STATUS_POLL_MS = 15_000;
const SYNCING_STATUS_POLL_MS = 3_000;

function refetchUnlessInvalidated(query: {
  state: { data: unknown; isInvalidated: boolean };
}): boolean {
  return query.state.data === undefined || !query.state.isInvalidated;
}

export function useNodeLinkStatusQuery(node: RestNode, enabled: boolean) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useQuery({
    queryKey: queryKeys.nodeLinkStatus(node.space_id, node.id),
    queryFn: () => getNodeLinkStatus(client, node.space_id, node.id),
    enabled,
    refetchInterval: (query) => linkStatusPollInterval(
      query.state.data,
      query.state.status === "error",
      hasInvalidatedNodeLinks(queryClient, node)
    )
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
    refetchOnMount: refetchUnlessInvalidated,
    refetchOnWindowFocus: refetchUnlessInvalidated,
    refetchOnReconnect: refetchUnlessInvalidated,
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
          space_pending: current?.space_pending ?? false,
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
      invalidateSpaceLinks(queryClient, spaceId);
    }
  });
}

export function projectionPollInterval(status: NodeLinkProjectionStatus | undefined): number | false {
  if (status?.status === "syncing") return SYNCING_STATUS_POLL_MS;
  if (status?.status === "pending" || status?.space_pending) return PENDING_STATUS_POLL_MS;
  return false;
}

export function linkStatusPollInterval(
  status: NodeLinkProjectionStatus | undefined,
  statusError: boolean,
  linksInvalidated: boolean
): number | false {
  return projectionPollInterval(status)
    || (statusError && linksInvalidated ? PENDING_STATUS_POLL_MS : false);
}

function hasInvalidatedNodeLinks(queryClient: QueryClient, node: RestNode): boolean {
  return (["outgoing", "incoming"] as const).some((direction) => (
    queryClient.getQueryState(
      queryKeys.nodeLinkList(node.space_id, node.id, direction)
    )?.isInvalidated === true
  ));
}
