import type { QueryClient, QueryKey } from "@tanstack/react-query";

import type { RestNode } from "../entities/node/model";
import { queryKeys } from "./queryKeys";

export function invalidateAuditEvents(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.auditEvents });
}

export function invalidateSpaceResources(queryClient: QueryClient, spaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.space(spaceId) });
}

export function invalidateSpace(queryClient: QueryClient, spaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.spaces, exact: true });
  invalidateSpaceResources(queryClient, spaceId);
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
