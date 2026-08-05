import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import {
  getLinkIndexState,
  getNodeLinks,
  requestLinkReindex,
  type LinkIndexState
} from "../../api/linkIndex";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";

export function useLinkIndexStateQuery(spaceId: string | null) {
  const client = useApiClient();
  return useQuery({
    queryKey: spaceId ? queryKeys.linkIndex(spaceId) : ["spaces", "none", "link-index"],
    queryFn: () => getLinkIndexState(client, spaceId!),
    enabled: Boolean(spaceId),
    refetchInterval: (query) => pollInterval(query.state.data)
  });
}

export function useNodeLinksQuery(spaceId: string | null, nodeId: string | null) {
  const client = useApiClient();
  return useQuery({
    queryKey: spaceId && nodeId
      ? queryKeys.nodeLinks(spaceId, nodeId)
      : ["spaces", "none", "link-index", "nodes", "none"],
    queryFn: () => getNodeLinks(client, spaceId!, nodeId!),
    enabled: Boolean(spaceId && nodeId),
    refetchInterval: (query) => pollInterval(query.state.data?.index)
  });
}

export function useRequestLinkReindexMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (spaceId: string) => requestLinkReindex(client, spaceId),
    meta: { silentError: true },
    onSuccess: (state, spaceId) => {
      queryClient.setQueryData(queryKeys.linkIndex(spaceId), state);
      void queryClient.invalidateQueries({ queryKey: queryKeys.linkIndex(spaceId) });
    }
  });
}

function pollInterval(state: LinkIndexState | undefined): number | false {
  if (state?.freshness === "failed") return POLLING.linkIndexFailedMs;
  return state?.freshness === "updating" || state?.freshness === "rebuilding"
    ? POLLING.linkIndexPendingMs
    : false;
}
