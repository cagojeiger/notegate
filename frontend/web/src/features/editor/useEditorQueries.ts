import { useCallback, useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { downloadFile, fetchFileBlob } from "../../api/files";
import { getNode, listChildren, resolveNodePath } from "../../api/nodes";
import { POLLING, withPollingJitter } from "../../api/polling";
import { invalidateSpaceResources } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { RestNode } from "../../api/types";
import type { MarkdownImageLoadResult } from "../../shared/lib/markdownLinks";
import { usePageVisible } from "../../shared/hooks/usePageVisible";
import { useUiStore } from "../../stores/uiStore";

export function useFolderChildrenStat(node: RestNode) {
  const client = useApiClient();
  return useQuery({ queryKey: [...queryKeys.children(node.space_id, node.id), "stat"], queryFn: () => listChildren(client, node.space_id, node.id) });
}

export function useFileDownload(node: RestNode) {
  const client = useApiClient();
  return async () => downloadFile(client, node.space_id, node.id, node.original_filename ?? node.name);
}

export function useMarkdownImageLoader(sourceNode: RestNode) {
  const client = useApiClient();
  const queryClient = useQueryClient();

  return useCallback(async (path: string): Promise<MarkdownImageLoadResult> => {
    let imageNode: RestNode;
    try {
      imageNode = await queryClient.fetchQuery({
        queryKey: queryKeys.markdownImageNode(sourceNode.space_id, path),
        queryFn: () => resolveNodePath(client, sourceNode.space_id, path),
        retry: false,
        staleTime: 5_000
      });
    } catch (error) {
      return { status: error instanceof ApiError && error.status === 404 ? "not-found" : "error" };
    }

    if (!isRenderableMarkdownImage(sourceNode.space_id, imageNode)) {
      return { status: "unsupported" };
    }

    try {
      const contentVersion = imageNode.content_sha256 ?? imageNode.updated_at;
      const blob = await queryClient.fetchQuery({
        queryKey: queryKeys.markdownImageBlob(imageNode.space_id, imageNode.id, contentVersion),
        queryFn: () => fetchFileBlob(client, imageNode.space_id, imageNode.id),
        retry: false,
        staleTime: Infinity
      });
      return { status: "loaded", blob };
    } catch {
      return { status: "error" };
    }
  }, [client, queryClient, sourceNode.space_id]);
}

export function useTextDocument(node: RestNode) {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.text(node.space_id, node.id), queryFn: () => readText(client, node.space_id, node.id) });
}

export function useNodeFreshness(node: RestNode) {
  const client = useApiClient();
  const pageVisible = usePageVisible();
  const refetchInterval = useMemo(() => withPollingJitter(POLLING.openedNodeMs, POLLING.openedNodeJitterMs), []);
  return useQuery({
    queryKey: queryKeys.node(node.space_id, node.id),
    queryFn: () => getNode(client, node.space_id, node.id),
    enabled: pageVisible,
    refetchInterval: pageVisible ? refetchInterval : false,
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
      updateGroupsNode({
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

function isRenderableMarkdownImage(sourceSpaceId: string, imageNode: RestNode): boolean {
  return (
    imageNode.space_id === sourceSpaceId &&
    imageNode.kind === "file" &&
    imageNode.encryption_mode !== "client" &&
    imageNode.media_type?.toLowerCase().startsWith("image/") === true
  );
}
