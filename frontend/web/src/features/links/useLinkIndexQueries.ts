import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import {
  getSpaceLinkIndex,
  listNodeLinkReferences,
  reindexSpaceLinks,
  syncNodeLinkIndex
} from "../../api/linkIndex";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import type { LinkReferenceDirection, LinkSyncStatus, RestNode } from "../../api/types";

export function useNodeLinkReferencesQuery(
  node: RestNode | null,
  direction: LinkReferenceDirection,
  snapshot: string | null
) {
  const client = useApiClient();
  return useInfiniteQuery({
    queryKey: node && snapshot
      ? queryKeys.nodeLinkReferences(node.space_id, node.id, direction, snapshot)
      : ["node-link-references", "none", direction],
    queryFn: ({ pageParam }) => listNodeLinkReferences(
      client,
      node!.space_id,
      node!.id,
      direction,
      pageParam
    ),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => (
      lastPage.page.has_more ? lastPage.page.next_cursor : undefined
    ),
    enabled: node !== null && snapshot !== null
  });
}

export function useSyncNodeLinkIndexMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ spaceId, nodeId }: { spaceId: string; nodeId: string }) => (
      syncNodeLinkIndex(client, spaceId, nodeId)
    ),
    onSuccess: (_response, { spaceId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaceLinkIndex(spaceId) });
    }
  });
}

export function useSpaceLinkIndexQuery(spaceId: string | null) {
  const client = useApiClient();
  return useQuery({
    queryKey: spaceId ? queryKeys.spaceLinkIndex(spaceId) : ["space-link-index", "none"],
    queryFn: () => getSpaceLinkIndex(client, spaceId!),
    enabled: spaceId !== null,
    refetchInterval: (query) => linkIndexPollInterval(query.state.data?.status)
  });
}

export function useReindexSpaceLinksMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (spaceId: string) => reindexSpaceLinks(client, spaceId),
    onSuccess: (_response, spaceId) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaceLinkIndex(spaceId) });
    }
  });
}

export function linkIndexPollInterval(status: LinkSyncStatus | undefined) {
  return status === "pending" || status === "syncing" || status === "retrying"
    ? POLLING.linkIndexPendingMs
    : false;
}
