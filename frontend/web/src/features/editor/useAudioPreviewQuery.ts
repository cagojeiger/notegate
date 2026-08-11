import { useQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { filePreviewStaleTime, getAudioPreviewUrl } from "../../api/files";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode } from "../../api/types";

const AUDIO_PREVIEW_CACHE_GC_MS = 15 * 60 * 1_000;

export function useAudioPreviewUrl(node: RestNode) {
  const client = useApiClient();
  return useQuery({
    queryKey: queryKeys.audioPreviewUrl(node.space_id, node.id),
    queryFn: () => getAudioPreviewUrl(client, node.space_id, node.id),
    enabled: canPreviewAudio(node),
    retry: false,
    gcTime: AUDIO_PREVIEW_CACHE_GC_MS,
    staleTime: (query) => filePreviewStaleTime(
      query.state.data?.expires_at ?? "",
      query.state.dataUpdatedAt
    )
  });
}

export function canPreviewAudio(node: RestNode): boolean {
  return node.kind === "file"
    && node.file_media_kind === "audio"
    && node.encryption_mode !== "client";
}
