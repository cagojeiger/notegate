import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import {
  batchListChildren,
  MAX_BATCH_CHILDREN_PARENTS
} from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type {
  BatchChildrenItem,
  ChildrenResponse
} from "../../api/types";

export function useTreeRestoreBatch(
  spaceId: string,
  rootNodeId: string,
  expandedFolderIds: ReadonlySet<string>
) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const childrenRevision =
    useQuery({
      queryKey: queryKeys.childrenRevision(spaceId),
      queryFn: () => 0,
      initialData: 0,
      staleTime: Number.POSITIVE_INFINITY
    }).data ?? 0;
  const parentIds = useMemo(
    () => [
      rootNodeId,
      ...[...expandedFolderIds]
        .filter((parentId) => parentId !== rootNodeId)
        .sort()
    ],
    [expandedFolderIds, rootNodeId]
  );
  const missingParentIds = parentIds.filter(
    (parentId) =>
      queryClient.getQueryData(queryKeys.children(spaceId, parentId)) ===
      undefined
  );
  const restoreKey = `${spaceId}:${childrenRevision}:${parentIds.join(",")}`;
  const failedRestoreKey = useRef<string | null>(null);
  const shouldBatch =
    missingParentIds.length > 1 && failedRestoreKey.current !== restoreKey;
  const requestKey = missingParentIds.join(",");
  // Keep the failed query dormant for its current tree revision, then give
  // React Query a fresh key when a later revision makes batching useful again.
  const retryEpoch =
    failedRestoreKey.current === null ? 0 : childrenRevision + 1;
  const [hydratedKey, setHydratedKey] = useState<string | null>(null);
  const batch = useQuery({
    queryKey: queryKeys.treeRestore(spaceId, retryEpoch, missingParentIds),
    queryFn: async () => {
      try {
        const responses = await Promise.all(
          chunk(missingParentIds, MAX_BATCH_CHILDREN_PARENTS).map((parentIds) =>
            batchListChildren(client, spaceId, parentIds)
          )
        );
        const results = responses.flatMap((response) => response.results);
        validateBatchResults(missingParentIds, results);
        return { childrenRevision, results };
      } catch (error) {
        failedRestoreKey.current = restoreKey;
        throw error;
      }
    },
    enabled: shouldBatch,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
    // Results are copied into the canonical per-folder caches and do not need
    // a second cache lifetime of their own.
    gcTime: 0
  });

  useEffect(() => {
    if (!shouldBatch || !batch.data) return;
    const currentRevision =
      queryClient.getQueryData<number>(
        queryKeys.childrenRevision(spaceId)
      ) ?? 0;
    if (currentRevision !== batch.data.childrenRevision) {
      failedRestoreKey.current = restoreKey;
      setHydratedKey(requestKey);
      return;
    }
    for (const result of batch.data.results) {
      if (
        result.status !== "ready" ||
        result.parent === null ||
        result.page === null
      ) {
        continue;
      }
      const page: ChildrenResponse = {
        parent: result.parent,
        children: result.children,
        page: result.page
      };
      queryClient.setQueryData(queryKeys.children(spaceId, result.parent_id), {
        pages: [page],
        pageParams: [null]
      });
    }
    setHydratedKey(requestKey);
  }, [batch.data, queryClient, requestKey, restoreKey, shouldBatch, spaceId]);

  // If the bounded batch fails, mount the existing
  // per-folder queries so the tree remains usable and the original error stays
  // visible to React Query's global error handling.
  return shouldBatch && !batch.isError && hydratedKey !== requestKey;
}

function validateBatchResults(
  parentIds: string[],
  results: BatchChildrenItem[]
) {
  if (
    results.length !== parentIds.length ||
    results.some((result, index) => result.parent_id !== parentIds[index])
  ) {
    throw new Error("Batch children response does not match the request");
  }
  for (const result of results) {
    if (
      result.status !== "ready" &&
      result.status !== "not_found" &&
      result.status !== "not_folder"
    ) {
      throw new Error("Batch children response has an unknown status");
    }
    if (
      result.status === "ready" &&
      (result.parent === null || result.page === null)
    ) {
      throw new Error("Ready batch children result is incomplete");
    }
  }
}

function chunk<T>(items: T[], size: number): T[][] {
  const chunks: T[][] = [];
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size));
  }
  return chunks;
}
