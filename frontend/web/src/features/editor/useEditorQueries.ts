import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { downloadFile } from "../../api/files";
import { getNode, listChildren } from "../../api/nodes";
import { POLLING, withPollingJitter } from "../../api/polling";
import { invalidateSpaceResources } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { RestNode } from "../../api/types";
import { usePageVisible } from "../../shared/hooks/usePageVisible";
import { useUiStore } from "../../stores/uiStore";
import type { OpenedNodeRef } from "../../stores/uiStoreReducers";

export function useFolderChildrenStat(node: RestNode) {
  const client = useApiClient();
  return useQuery({ queryKey: [...queryKeys.children(node.space_id, node.id), "stat"], queryFn: () => listChildren(client, node.space_id, node.id) });
}

export function useFileDownload(node: RestNode) {
  const client = useApiClient();
  return async () => downloadFile(client, node.space_id, node.id, node.original_filename ?? node.name);
}

export function useTextDocument(node: RestNode) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.text(node.space_id, node.id), queryFn: () => readText(client, node.space_id, node.id) });
}

export function useOpenedNodeQuery(nodeRef: OpenedNodeRef | null) {
  const client = useApiClient();
  const pageVisible = usePageVisible();
  const refetchInterval = useMemo(() => withPollingJitter(POLLING.openedNodeMs, POLLING.openedNodeJitterMs), []);
  return useQuery({
    queryKey: nodeRef ? queryKeys.node(nodeRef.spaceId, nodeRef.nodeId) : ["opened-node", "none"],
    queryFn: () => {
      if (!nodeRef) throw new Error("No opened node");
      return getNode(client, nodeRef.spaceId, nodeRef.nodeId);
    },
    enabled: Boolean(nodeRef) && pageVisible,
    refetchInterval: pageVisible ? refetchInterval : false,
    retry: (failureCount, error) => !(error instanceof ApiError && error.status === 404) && failureCount < 3
  });
}

export function useOpenedNodeCache(nodeRef: OpenedNodeRef | null) {
  return useQuery<RestNode>({
    queryKey: nodeRef ? queryKeys.node(nodeRef.spaceId, nodeRef.nodeId) : ["opened-node", "none"],
    queryFn: async () => {
      throw new Error("Opened node cache subscriptions do not fetch");
    },
    enabled: false
  });
}

export function useSaveTextDocument(node: RestNode, draft: string, sha: string | undefined, onSaved: () => void, onConflict: () => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const setSaveState = useUiStore((state) => state.setSaveState);
  return useMutation({
    meta: { silentError: true },
    mutationFn: (force: boolean) => replaceText(client, node.space_id, node.id, draft, force ? undefined : sha),
    onMutate: () => setSaveState("saving"),
    onSuccess: (response) => {
      queryClient.setQueryData<RestNode>(queryKeys.node(node.space_id, node.id), {
        ...node,
        content_sha256: response.text.content_sha256,
        byte_len: response.text.byte_len,
        line_count: response.text.line_count,
        updated_by: response.text.updated_by,
        updated_at: response.text.updated_at
      });
      setSaveState("saved");
      showToast("Saved");
      onSaved();
      invalidateSpaceResources(queryClient, node.space_id);
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409) {
        onConflict();
        setSaveState("conflict");
      } else {
        setSaveState("error");
      }
    }
  });
}
