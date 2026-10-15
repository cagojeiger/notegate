import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { updateNodeCaches } from "../../api/nodeCache";
import { getNode } from "../../api/nodes";
import { invalidateRecentNodes, invalidateSpaceLinks } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { ReadTextResponse, RestNode, TextResponse } from "../../api/types";
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

export type TextSaveRequest = {
  node: RestNode;
  content: string;
  expectedSha256?: string;
  editSessionId: string;
  force: boolean;
};

export function useSaveTextDocument(
  onSaved: (saved: { request: TextSaveRequest; response: TextResponse }) => void,
  onConflict: (request: TextSaveRequest) => void
) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const setSaveState = useUiStore((state) => state.setSaveState);
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  return useMutation({
    meta: { silentError: true },
    mutationFn: async (request: TextSaveRequest) => {
      const response = await replaceText(
        client,
        request.node.space_id,
        request.node.id,
        request.content,
        request.force ? undefined : request.expectedSha256,
        request.editSessionId
      );
      return { response, request };
    },
    onMutate: async ({ node }) => {
      setSaveState("saving");
      await queryClient.cancelQueries({
        queryKey: queryKeys.text(node.space_id, node.id),
        exact: true
      });
    },
    onSuccess: async ({ response, request }) => {
      const { node, content } = request;
      // A refresh can start while the PUT is pending; do not let it replace this save.
      await queryClient.cancelQueries({ queryKey: queryKeys.text(node.space_id, node.id), exact: true });
      const cachedNode = queryClient.getQueryData<RestNode>(queryKeys.node(node.space_id, node.id));
      const currentNode = cachedNode ?? node;
      // Saving text must not roll back a concurrent rename, move, or policy change.
      const textUpdate = {
        content_sha256: response.text.content_sha256,
        byte_len: response.text.byte_len,
        line_count: response.text.line_count,
        ...(Date.parse(currentNode.updated_at) > Date.parse(response.text.updated_at) ? {} : {
          updated_by: response.text.updated_by,
          updated_at: response.text.updated_at
        })
      };
      queryClient.setQueryData<ReadTextResponse>(
        queryKeys.text(node.space_id, node.id),
        {
          node: { ...response.node, path: cachedNode?.path ?? response.node.path },
          text: {
            ...response.text,
            storage_format: "plain",
            content,
            start_line: 1,
            end_line: response.text.line_count,
            returned_lines: response.text.line_count,
            truncated: false,
            next_start_line: null
          }
        }
      );
      const updatedNode = { ...currentNode, ...textUpdate };
      updateGroupsNode(updatedNode);
      updateNodeCaches(queryClient, updatedNode, (current) => ({ ...current, ...textUpdate }));
      setSaveState("saved");
      showToast("Saved");
      onSaved({ response, request });
      invalidateRecentNodes(queryClient, node.space_id);
      invalidateSpaceLinks(queryClient, node.space_id);
    },
    onError: (error, request) => {
      const { node } = request;
      if (error instanceof ApiError && error.status === 409) {
        onConflict(request);
        setSaveState("conflict");
        return;
      }
      setSaveState("error");
      if (error instanceof ApiError && error.kind === "text_revision_storage_full") {
        showToast(error.message);
        return;
      }
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
