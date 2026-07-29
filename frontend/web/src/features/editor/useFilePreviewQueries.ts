import { useCallback, useEffect, useMemo } from "react";
import { queryOptions, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import type { ApiClient } from "../../api/client";
import { ApiError } from "../../api/errors";
import { filePreviewStaleTime, getFilePreviewUrl } from "../../api/files";
import { updateExistingNodeCaches, updateNodeCaches } from "../../api/nodeCache";
import { getNode } from "../../api/nodes";
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
  const canonicalState = queryClient.getQueryState(queryKeys.node(node.space_id, node.id));
  const previewQuery = useQuery({
    ...filePreviewQueryOptions(client, queryClient, node, previewKind ?? "image"),
    enabled: previewKind !== null
  });

  useEffect(() => {
    const preview = previewQuery.data;
    if (!preview || previewKind === null) return;
    refreshDiscoveredPreviewState(queryClient, node, preview.media_type, previewKind);
    if (previewKind !== "image") return;

    const canonicalNodeKey = queryKeys.node(node.space_id, node.id);
    const currentCanonicalNode = queryClient.getQueryData<RestNode>(canonicalNodeKey);
    const currentCanonicalState = queryClient.getQueryState(canonicalNodeKey);
    if (!currentCanonicalNode
      || currentCanonicalState?.status !== "success"
      || currentCanonicalState.fetchStatus !== "idle"
      || currentCanonicalState.isInvalidated) return;

    queryClient.setQueryData<BatchFilePreviewItem>(
      queryKeys.markdownImagePreview(node.space_id, currentCanonicalNode.path),
      {
        path: currentCanonicalNode.path,
        status: "ready",
        node_id: currentCanonicalNode.id,
        media_type: preview.media_type,
        url: preview.url,
        expires_at: preview.expires_at
      }
    );
  }, [
    canonicalState,
    node,
    previewKind,
    previewQuery.data,
    queryClient
  ]);

  return previewQuery;
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
        return preview;
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) {
          await refreshPreviewNodeAfterNotFound(client, queryClient, node);
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

async function refreshPreviewNodeAfterNotFound(
  client: ApiClient,
  queryClient: QueryClient,
  node: RestNode
) {
  const refreshedNode = await queryClient.fetchQuery({
    queryKey: queryKeys.node(node.space_id, node.id),
    queryFn: () => getNode(client, node.space_id, node.id),
    staleTime: 0
  }).catch(() => null);
  if (refreshedNode) updateNodeCaches(queryClient, refreshedNode, () => refreshedNode);
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

  const canonicalKey = queryKeys.node(node.space_id, node.id);
  const canonicalState = queryClient.getQueryState(canonicalKey);
  const keepCanonicalInvalidated = canonicalState !== undefined && (
    canonicalState.status !== "success"
    || canonicalState.fetchStatus !== "idle"
    || canonicalState.isInvalidated
  );
  updateExistingNodeCaches(queryClient, node.space_id, node.id, (current) => ({
    ...current,
    detected_media_type: detectedMediaType ?? current.detected_media_type ?? node.detected_media_type,
    preview_available: previewAvailable,
    file_preview_kind: nextPreviewKind
  }));
  if (keepCanonicalInvalidated) {
    void queryClient.invalidateQueries({
      queryKey: canonicalKey,
      exact: true,
      refetchType: "none"
    });
  }
}
