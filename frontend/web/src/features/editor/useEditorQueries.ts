import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { updateNodeCaches } from "../../api/nodeCache";
import { getNode } from "../../api/nodes";
import { invalidateRecentNodes } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { ReadTextResponse, RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { useNodeChildrenQuery } from "../nodes/useNodeQueries";

export function useFolderChildrenStat(node: RestNode) {
  const query = useNodeChildrenQuery(node.space_id, node.id, true);
  return { ...query, data: query.data?.pages[0] };
}

export function useTextDocument(node: RestNode) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.text(node.space_id, node.id),
    queryFn: () => readText(client, node.space_id, node.id),
    // Active-Space change sync owns focus refresh; reconnect remains a direct fallback.
    refetchOnWindowFocus: false
  });
}

export function useNodeFreshness(node: RestNode) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.node(node.space_id, node.id),
    queryFn: () => getNode(client, node.space_id, node.id),
    // Active-Space change sync owns focus refresh; reconnect remains a direct fallback.
    refetchOnWindowFocus: false,
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
    mutationFn: async (force: boolean) => {
      const submittedDraft = draft;
      const response = await replaceText(
        client,
        node.space_id,
        node.id,
        submittedDraft,
        force ? undefined : sha
      );
      return { response, submittedDraft };
    },
    onMutate: async () => {
      setSaveState("saving");
      await queryClient.cancelQueries({
        queryKey: queryKeys.text(node.space_id, node.id),
        exact: true
      });
    },
    onSuccess: ({ response, submittedDraft }) => {
      queryClient.setQueryData<ReadTextResponse>(
        queryKeys.text(node.space_id, node.id),
        {
          node: response.node,
          text: {
            ...response.text,
            storage_format: "plain",
            content: submittedDraft,
            start_line: 1,
            end_line: response.text.line_count,
            returned_lines: response.text.line_count,
            truncated: false,
            next_start_line: null
          }
        }
      );
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
      invalidateRecentNodes(queryClient, node.space_id);
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409) {
        onConflict();
        setSaveState("conflict");
        return;
      }
      setSaveState("error");
      if (
        error instanceof ApiError
        && error.status === 423
        && (error.kind === "node_write_locked" || error.kind === "subtree_write_locked")
      ) {
        showToast(error.message);
        void queryClient.invalidateQueries({
          queryKey: queryKeys.node(node.space_id, node.id),
          exact: true
        });
      }
    }
  });
}
