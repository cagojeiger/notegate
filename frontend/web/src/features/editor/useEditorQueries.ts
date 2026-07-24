import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { downloadFile } from "../../api/files";
import { updateNodeCaches } from "../../api/nodeCache";
import { getNode, listChildren } from "../../api/nodes";
import { invalidateRecentNodes, invalidateText } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";

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

export function useNodeFreshness(node: RestNode) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.node(node.space_id, node.id),
    queryFn: () => getNode(client, node.space_id, node.id),
    retry: (failureCount, error) => !(error instanceof ApiError && error.status === 404) && failureCount < 3
  });
}

export function useSaveTextDocument(node: RestNode, draft: string, sha: string | undefined, onSaved: () => void, onConflict: () => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const setSaveState = useUiStore((state) => state.setSaveState);
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  return useMutation({
    meta: { silentError: true },
    mutationFn: (force: boolean) => replaceText(client, node.space_id, node.id, draft, force ? undefined : sha),
    onMutate: () => setSaveState("saving"),
    onSuccess: (response) => {
      const updatedNode = {
        ...node,
        content_sha256: response.text.content_sha256,
        byte_len: response.text.byte_len,
        line_count: response.text.line_count,
        updated_by: response.text.updated_by,
        updated_at: response.text.updated_at
      };
      updateGroupsNode(updatedNode);
      updateNodeCaches(queryClient, updatedNode, () => updatedNode);
      setSaveState("saved");
      showToast("Saved");
      onSaved();
      invalidateText(queryClient, node.space_id, node.id);
      invalidateRecentNodes(queryClient, node.space_id);
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
