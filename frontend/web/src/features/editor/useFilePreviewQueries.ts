import { useCallback } from "react";
import { queryOptions, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import type { ApiClient } from "../../api/client";
import { ApiError } from "../../api/errors";
import { filePreviewStaleTime, getFilePreviewUrl } from "../../api/files";
import { resolveNodePath } from "../../api/nodes";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";
import type { MarkdownImageLoadOptions, MarkdownImageLoadResult } from "../../shared/lib/markdownLinks";

const FILE_PREVIEW_CACHE_GC_MS = 15 * 60 * 1_000;

export function useFilePreviewUrl(node: RestNode) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useQuery({
    ...filePreviewQueryOptions(client, queryClient, node),
    enabled: isImagePreviewCandidate(node)
  });
}

export function useMarkdownImageLoader(sourceNode: RestNode) {
  const client = useApiClient();
  const queryClient = useQueryClient();

  return useCallback(async (path: string, options: MarkdownImageLoadOptions = {}): Promise<MarkdownImageLoadResult> => {
    let imageNode: RestNode;
    try {
      imageNode = await queryClient.fetchQuery({
        queryKey: queryKeys.markdownImageNode(sourceNode.space_id, path),
        queryFn: () => resolveNodePath(client, sourceNode.space_id, path),
        retry: false,
        staleTime: POLLING.spaceChangesMs
      });
    } catch (error) {
      return { status: error instanceof ApiError && error.status === 404 ? "not-found" : "error" };
    }

    if (!isRenderableMarkdownImage(sourceNode.space_id, imageNode)) {
      return { status: "unsupported" };
    }

    try {
      const query = filePreviewQueryOptions(client, queryClient, imageNode);
      const preview = await queryClient.fetchQuery(options.forceRefresh ? { ...query, staleTime: 0 } : query);
      return { status: "loaded", url: preview.url };
    } catch (error) {
      return {
        status: error instanceof ApiError && error.status === 404 ? "unsupported" : "error"
      };
    }
  }, [client, queryClient, sourceNode.space_id]);
}

function isRenderableMarkdownImage(sourceSpaceId: string, imageNode: RestNode): boolean {
  return imageNode.space_id === sourceSpaceId && isImagePreviewCandidate(imageNode);
}

function isImagePreviewCandidate(node: RestNode): boolean {
  if (node.kind !== "file" || node.encryption_mode === "client") return false;
  if (node.preview_available !== undefined) return node.preview_available;
  return true;
}

function filePreviewQueryOptions(client: ApiClient, queryClient: QueryClient, node: RestNode) {
  return queryOptions({
    queryKey: queryKeys.filePreviewUrl(node.space_id, node.id),
    queryFn: async () => {
      try {
        const preview = await getFilePreviewUrl(client, node.space_id, node.id);
        refreshDiscoveredPreviewState(queryClient, node, preview.media_type);
        return preview;
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) {
          refreshDiscoveredPreviewState(queryClient, node, null);
        }
        throw error;
      }
    },
    retry: false,
    gcTime: FILE_PREVIEW_CACHE_GC_MS,
    staleTime: (query) => filePreviewStaleTime(
      query.state.data?.expires_at ?? "",
      query.state.dataUpdatedAt
    )
  });
}

function refreshDiscoveredPreviewState(
  queryClient: QueryClient,
  node: RestNode,
  detectedMediaType: string | null
) {
  if (node.preview_available !== undefined) return;
  const updatePreviewFields = (current: RestNode | undefined): RestNode => ({
    ...(current ?? node),
    detected_media_type: detectedMediaType ?? current?.detected_media_type ?? node.detected_media_type,
    preview_available: detectedMediaType !== null
  });
  queryClient.setQueryData<RestNode>(queryKeys.node(node.space_id, node.id), updatePreviewFields);
  queryClient.setQueryData<RestNode>(
    queryKeys.markdownImageNode(node.space_id, node.path),
    updatePreviewFields
  );
  void queryClient.invalidateQueries({ queryKey: queryKeys.node(node.space_id, node.id) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.recent(node.space_id) });
  if (node.parent_id) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.children(node.space_id, node.parent_id) });
  }
}
