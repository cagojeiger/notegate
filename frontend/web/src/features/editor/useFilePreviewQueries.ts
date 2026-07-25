import { useCallback, useMemo } from "react";
import { queryOptions, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import type { ApiClient } from "../../api/client";
import { ApiError } from "../../api/errors";
import { filePreviewStaleTime, getFilePreviewUrl } from "../../api/files";
import { updateNodeCaches } from "../../api/nodeCache";
import { POLLING } from "../../api/polling";
import { queryKeys } from "../../api/queryKeys";
import type { BatchFilePreviewItem, FilePreviewKind, RestNode } from "../../api/types";
import type { MarkdownImageLoadOptions, MarkdownImageLoadResult } from "../../shared/lib/markdownLinks";
import { createMarkdownPreviewBatcher } from "./markdownPreviewBatcher";

const FILE_PREVIEW_CACHE_GC_MS = 15 * 60 * 1_000;

export function useFilePreviewUrl(node: RestNode) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const previewKind = filePreviewKindForNode(node);
  return useQuery({
    ...filePreviewQueryOptions(client, queryClient, node, previewKind ?? "image"),
    enabled: previewKind !== null
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

export function filePreviewKindForNode(node: RestNode): FilePreviewKind | null {
  if (node.kind !== "file" || node.encryption_mode === "client") return null;
  if (node.file_preview_kind) return node.file_preview_kind;
  if (node.preview_available === true) return "image";
  if (node.preview_available === false) return null;
  if (node.detected_media_type === "application/pdf" || node.media_type === "application/pdf") {
    return "pdf";
  }
  return "image";
}

function filePreviewQueryOptions(
  client: ApiClient,
  queryClient: QueryClient,
  node: RestNode,
  previewKind: FilePreviewKind
) {
  return queryOptions({
    queryKey: queryKeys.filePreviewUrl(node.space_id, node.id, previewKind),
    queryFn: async () => {
      try {
        const preview = await getFilePreviewUrl(client, node.space_id, node.id, previewKind);
        refreshDiscoveredPreviewState(queryClient, node, preview.media_type, previewKind);
        return preview;
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) {
          refreshDiscoveredPreviewState(queryClient, node, null, null);
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
  detectedMediaType: string | null,
  previewKind: FilePreviewKind | null
) {
  const previewAvailable = previewKind === "image";
  const nextPreviewKind = previewKind ?? undefined;
  if (node.preview_available === previewAvailable
    && node.file_preview_kind === nextPreviewKind
    && (!detectedMediaType || node.detected_media_type === detectedMediaType)) return;

  updateNodeCaches(queryClient, node, (current) => ({
    ...current,
    detected_media_type: detectedMediaType ?? current.detected_media_type ?? node.detected_media_type,
    preview_available: previewAvailable,
    file_preview_kind: nextPreviewKind
  }));
}
