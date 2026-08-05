import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import {
  getNodeLinkIndex,
  getSpaceLinkIndex,
  reindexSpaceLinks,
  syncNodeLinkIndex
} from "../../api/linkIndex";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import type { LinkSyncStatus, RestNode } from "../../api/types";

export function useNodeLinkIndexQuery(node: RestNode | null) {
  const client = useApiClient();
  return useQuery({
    queryKey: node ? queryKeys.nodeLinkIndex(node.space_id, node.id) : ["node-link-index", "none"],
    queryFn: () => getNodeLinkIndex(client, node!.space_id, node!.id),
    enabled: node !== null,
    refetchInterval: (query) => pollInterval(query.state.data?.status)
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
    refetchInterval: (query) => pollInterval(query.state.data?.status)
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

function pollInterval(status: LinkSyncStatus | undefined) {
  return status && status !== "up_to_date" ? POLLING.linkIndexPendingMs : false;
}
