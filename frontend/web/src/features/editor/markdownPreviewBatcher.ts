import type { QueryClient } from "@tanstack/react-query";

import type { ApiClient } from "../../api/client";
import { batchResolveFilePreviews } from "../../api/files";
import { updateExistingNodeCaches } from "../../api/nodeCache";
import { queryKeys } from "../../api/queryKeys";
import type { BatchFilePreviewItem } from "../../api/types";

const MAX_BATCH_PATHS = 64;
const MAX_BATCH_PATH_BYTES = 16 * 1024;

type PendingRequest = {
  resolve: (item: BatchFilePreviewItem) => void;
  reject: (error: unknown) => void;
};

export function createMarkdownPreviewBatcher(
  client: ApiClient,
  queryClient: QueryClient,
  spaceId: string
) {
  const pending = new Map<string, PendingRequest[]>();
  let scheduled = false;

  return function load(path: string): Promise<BatchFilePreviewItem> {
    const promise = new Promise<BatchFilePreviewItem>((resolve, reject) => {
      const requests = pending.get(path) ?? [];
      requests.push({ resolve, reject });
      pending.set(path, requests);
    });
    if (!scheduled) {
      scheduled = true;
      queueMicrotask(() => {
        scheduled = false;
        void flush();
      });
    }
    return promise;
  };

  async function flush() {
    const entries = [...pending.entries()];
    pending.clear();
    for (const batch of partition(entries)) {
      await resolveBatch(batch);
    }
  }

  async function resolveBatch(batch: Array<[string, PendingRequest[]]>) {
    try {
      const response = await batchResolveFilePreviews(
        client,
        spaceId,
        batch.map(([path]) => path)
      );
      if (response.results.length !== batch.length) {
        throw new Error("batch preview response length mismatch");
      }
      const orderedItems = batch.map(([path], index) => {
        const item = response.results[index];
        if (!item || item.path !== path) throw new Error("batch preview response order mismatch");
        return item;
      });
      batch.forEach(([, requests], index) => {
        const item = orderedItems[index];
        cacheBatchResult(queryClient, spaceId, item);
        requests.forEach(({ resolve }) => resolve(item));
      });
    } catch (error) {
      batch.forEach(([, requests]) => {
        requests.forEach(({ reject }) => reject(error));
      });
    }
  }
}

function partition(entries: Array<[string, PendingRequest[]]>) {
  const batches: Array<Array<[string, PendingRequest[]]>> = [];
  let batch: Array<[string, PendingRequest[]]> = [];
  let bytes = 0;

  for (const entry of entries) {
    const pathBytes = new TextEncoder().encode(entry[0]).byteLength;
    if (batch.length > 0
      && (batch.length === MAX_BATCH_PATHS || bytes + pathBytes > MAX_BATCH_PATH_BYTES)) {
      batches.push(batch);
      batch = [];
      bytes = 0;
    }
    batch.push(entry);
    bytes += pathBytes;
  }
  if (batch.length > 0) batches.push(batch);
  return batches;
}

function cacheBatchResult(
  queryClient: QueryClient,
  spaceId: string,
  item: BatchFilePreviewItem
) {
  if (!item.node_id) return;
  if (item.status === "ready" && item.url && item.media_type && item.expires_at) {
    queryClient.setQueryData(queryKeys.filePreviewUrl(spaceId, item.node_id), {
      url: item.url,
      media_type: item.media_type,
      expires_at: item.expires_at
    });
  }
  const mediaType = item.media_type;
  if ((item.status === "ready" || item.status === "unsupported") && mediaType) {
    updateExistingNodeCaches(queryClient, spaceId, item.node_id, (current) => ({
      ...current,
      detected_media_type: mediaType,
      preview_available: item.status === "ready"
    }));
  }
}
