import { useCallback, useMemo } from "react";
import { queryOptions, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import type { ApiClient } from "../../api/client";
import { ApiError } from "../../api/errors";
import { filePreviewStaleTime, getFilePreviewUrl } from "../../api/files";
import { updateNodeCaches } from "../../api/nodeCache";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import type { BatchFilePreviewItem, RestNode } from "../../api/types";
import type { MarkdownImageLoadOptions, MarkdownImageLoadResult } from "../../shared/lib/markdownLinks";
import { createMarkdownPreviewBatcher } from "./markdownPreviewBatcher";

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
  const batchLoad = useMemo(
    () => createMarkdownPreviewBatcher(client, queryClient, sourceNode.space_id),
    [client, queryClient, sourceNode.space_id]
  );

  return useCallback(async (path: string, options: MarkdownImageLoadOptions = {}): Promise<MarkdownImageLoadResult> => {
    try {
      const query = markdownPreviewQueryOptions(
        sourceNode.space_id,
        path,
        batchLoad
      );
      const result = await queryClient.fetchQuery(
        options.forceRefresh ? { ...query, staleTime: 0 } : query
      );
      return markdownImageResult(result);
    } catch {
      return { status: "error" };
    }
  }, [batchLoad, queryClient, sourceNode.space_id]);
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

function markdownPreviewQueryOptions(
  spaceId: string,
  path: string,
  batchLoad: (path: string) => Promise<BatchFilePreviewItem>
) {
  return queryOptions({
    queryKey: queryKeys.markdownImagePreview(spaceId, path),
    queryFn: () => batchLoad(path),
    retry: false,
    gcTime: FILE_PREVIEW_CACHE_GC_MS,
    staleTime: (query) => {
      const result = query.state.data;
      if (result?.status === "ready" && result.expires_at) {
        return filePreviewStaleTime(result.expires_at, query.state.dataUpdatedAt);
      }
      return POLLING.spaceChangesMs;
    }
  });
}

function markdownImageResult(result: BatchFilePreviewItem): MarkdownImageLoadResult {
  if (result.status === "ready" && result.url) {
    return { status: "loaded", url: result.url };
  }
  if (result.status === "not_found") return { status: "not-found" };
  if (result.status === "unsupported") return { status: "unsupported" };
  return { status: "error" };
}

function refreshDiscoveredPreviewState(
  queryClient: QueryClient,
  node: RestNode,
  detectedMediaType: string | null
) {
  if (node.preview_available !== undefined) return;
  updateNodeCaches(queryClient, node, (current) => ({
    ...current,
    detected_media_type: detectedMediaType ?? current.detected_media_type ?? node.detected_media_type,
    preview_available: detectedMediaType !== null
  }));
}
