import type { QueryClient, QueryKey } from "@tanstack/react-query";

import type { RestNode } from "./types";
import { queryKeys } from "./queryKeys";

export function invalidateAuditEvents(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.auditEvents });
}

export function invalidateNodeLists(
  queryClient: QueryClient,
  spaceId: string,
  parentIds: Iterable<string | null | undefined>
) {
  invalidateRecentNodes(queryClient, spaceId);
  for (const parentId of new Set([...parentIds].filter((id): id is string => Boolean(id)))) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.children(spaceId, parentId) });
  }
}

export function invalidateRecentNodes(queryClient: QueryClient, spaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.recent(spaceId), exact: true });
}

export function invalidateFolderSubtree(queryClient: QueryClient, spaceId: string) {
  invalidateRecentNodes(queryClient, spaceId);
  void queryClient.invalidateQueries({ queryKey: queryKeys.childrenFamily(spaceId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.nodes(spaceId) });
  removeMarkdownImageNodeQueries(queryClient, spaceId);
}

export function invalidateText(queryClient: QueryClient, spaceId: string, nodeId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.text(spaceId, nodeId), exact: true });
}

export function removeMarkdownImageNodeQueries(queryClient: QueryClient, spaceId: string) {
  queryClient.removeQueries({ queryKey: queryKeys.markdownImageNodes(spaceId) });
}

export function invalidateSpaceResources(queryClient: QueryClient, spaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.space(spaceId) });
}

export function removeDeletedNodeQueries(
  queryClient: QueryClient,
  node: Pick<RestNode, "id" | "space_id" | "kind" | "path">,
  recursive: boolean
) {
  const previewQueryKey = recursive && node.kind === "folder"
    ? queryKeys.filePreviewUrls(node.space_id)
    : queryKeys.filePreviewUrl(node.space_id, node.id);

  return cancelAndRemoveQueries(queryClient, [
    queryKeys.node(node.space_id, node.id),
    queryKeys.text(node.space_id, node.id),
    queryKeys.file(node.space_id, node.id),
    queryKeys.metadata(node.space_id, node.id),
    queryKeys.markdownImageNode(node.space_id, node.path),
    previewQueryKey
  ]);
}

export function removeDeletedSpaceQueries(queryClient: QueryClient, spaceId: string) {
  return cancelAndRemoveQueries(queryClient, [
    queryKeys.space(spaceId),
    queryKeys.filePreviewUrls(spaceId)
  ]);
}

async function cancelAndRemoveQueries(queryClient: QueryClient, keys: QueryKey[]) {
  await Promise.all(keys.map((queryKey) => queryClient.cancelQueries({ queryKey })));
  keys.forEach((queryKey) => queryClient.removeQueries({ queryKey }));
}
