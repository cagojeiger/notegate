import type { QueryClient, QueryKey } from "@tanstack/react-query";

import type { FileChangeDelta, RestNode } from "./types";
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
  invalidateParentChildren(queryClient, spaceId, parentIds);
}

export function invalidateRecentNodes(queryClient: QueryClient, spaceId: string) {
  void queryClient.resetQueries({ queryKey: queryKeys.recent(spaceId), exact: true });
}

export function invalidateFolderSubtree(queryClient: QueryClient, spaceId: string) {
  invalidateRecentNodes(queryClient, spaceId);
  invalidateAllChildren(queryClient, spaceId);
  void queryClient.invalidateQueries({ queryKey: queryKeys.nodes(spaceId) });
  removeMarkdownImageQueries(queryClient, spaceId);
}

export async function applyExternalFileChanges(
  queryClient: QueryClient,
  spaceId: string,
  changes: FileChangeDelta[]
) {
  if (changes.length === 0) return;

  const nodeIds = new Set<string>();
  const textIds = new Set<string>();
  const metadataIds = new Set<string>();
  const parentIds = new Set<string>();
  let missingParent = false;
  let subtreeChanged = false;
  let subtreeDeleted = false;
  let pathChanged = false;

  void queryClient.invalidateQueries({
    queryKey: queryKeys.fileChangeEventsFamily(spaceId)
  });

  for (const change of changes) {
    subtreeChanged ||= change.subtree_changed;
    subtreeDeleted ||= change.subtree_changed && change.op_type === "item.delete";
    pathChanged ||= change.path_changed || change.op_type === "item.delete";
    if (!change.parent_scope_known) {
      missingParent = true;
    } else {
      change.affected_parent_ids.forEach((id) => parentIds.add(id));
    }
    if (change.op_type === "item.delete") {
      await removeExternalDeletedNode(queryClient, spaceId, change);
      continue;
    }
    if (!change.node_id) continue;
    nodeIds.add(change.node_id);
    if (change.op_type.startsWith("text.")) textIds.add(change.node_id);
    if (change.op_type.startsWith("metadata.")) metadataIds.add(change.node_id);
  }

  if (!subtreeChanged) {
    for (const nodeId of nodeIds) {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.node(spaceId, nodeId),
        exact: true
      });
    }
  }
  for (const nodeId of textIds) invalidateText(queryClient, spaceId, nodeId);
  for (const nodeId of metadataIds) {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.metadata(spaceId, nodeId),
      exact: true
    });
  }

  if (subtreeDeleted) {
    await cancelAndRemoveQueries(queryClient, [
      queryKeys.texts(spaceId),
      queryKeys.metadataFamily(spaceId),
      queryKeys.files(spaceId)
    ]);
  }

  if (subtreeChanged) {
    invalidateFolderSubtree(queryClient, spaceId);
    return;
  }

  invalidateRecentNodes(queryClient, spaceId);
  if (missingParent) {
    invalidateAllChildren(queryClient, spaceId);
  } else {
    invalidateParentChildren(queryClient, spaceId, parentIds);
  }
  if (pathChanged) {
    removeMarkdownImageQueries(queryClient, spaceId);
  }
}

export async function invalidateFileSyncFallback(queryClient: QueryClient, spaceId: string) {
  invalidateFolderSubtree(queryClient, spaceId);
  void queryClient.invalidateQueries({
    queryKey: queryKeys.fileChangeEventsFamily(spaceId)
  });
  void queryClient.invalidateQueries({ queryKey: queryKeys.texts(spaceId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.metadataFamily(spaceId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.files(spaceId) });
  await cancelAndRemoveQueries(queryClient, [queryKeys.filePreviewUrls(spaceId)]);
}

export function invalidateText(queryClient: QueryClient, spaceId: string, nodeId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.text(spaceId, nodeId), exact: true });
}

export function removeMarkdownImageQueries(queryClient: QueryClient, spaceId: string) {
  queryClient.removeQueries({ queryKey: queryKeys.markdownImagePreviews(spaceId) });
}

export function removeMarkdownImagePreviewQuery(
  queryClient: QueryClient,
  spaceId: string,
  path: string
) {
  queryClient.removeQueries({
    queryKey: queryKeys.markdownImagePreview(spaceId, path),
    exact: true
  });
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
    queryKeys.markdownImagePreview(node.space_id, node.path),
    previewQueryKey
  ]);
}

export function removeDeletedSpaceQueries(queryClient: QueryClient, spaceId: string) {
  return cancelAndRemoveQueries(queryClient, [
    queryKeys.space(spaceId),
    queryKeys.filePreviewUrls(spaceId)
  ]);
}

async function removeExternalDeletedNode(
  queryClient: QueryClient,
  spaceId: string,
  change: FileChangeDelta
) {
  if (!change.node_id) return;
  const cachedNode = queryClient.getQueryData<RestNode>(
    queryKeys.node(spaceId, change.node_id)
  );
  if (cachedNode) {
    await removeDeletedNodeQueries(
      queryClient,
      cachedNode,
      change.subtree_changed
    );
    return;
  }

  const previewKey = change.subtree_changed
    ? queryKeys.filePreviewUrls(spaceId)
    : queryKeys.filePreviewUrl(spaceId, change.node_id);
  await cancelAndRemoveQueries(queryClient, [
    queryKeys.node(spaceId, change.node_id),
    queryKeys.text(spaceId, change.node_id),
    queryKeys.file(spaceId, change.node_id),
    queryKeys.metadata(spaceId, change.node_id),
    previewKey
  ]);
}

async function cancelAndRemoveQueries(queryClient: QueryClient, keys: QueryKey[]) {
  await Promise.all(keys.map((queryKey) => queryClient.cancelQueries({ queryKey })));
  keys.forEach((queryKey) => queryClient.removeQueries({ queryKey }));
}

function invalidateParentChildren(
  queryClient: QueryClient,
  spaceId: string,
  parentIds: Iterable<string | null | undefined>
) {
  const uniqueParentIds = new Set(
    [...parentIds].filter((id): id is string => Boolean(id))
  );
  if (uniqueParentIds.size === 0) return;
  advanceChildrenRevision(queryClient, spaceId);
  for (const parentId of uniqueParentIds) {
    void queryClient.resetQueries({
      queryKey: queryKeys.children(spaceId, parentId)
    });
  }
}

function invalidateAllChildren(queryClient: QueryClient, spaceId: string) {
  advanceChildrenRevision(queryClient, spaceId);
  void queryClient.resetQueries({
    queryKey: queryKeys.childrenFamily(spaceId)
  });
}

function advanceChildrenRevision(queryClient: QueryClient, spaceId: string) {
  queryClient.setQueryData<number>(
    queryKeys.childrenRevision(spaceId),
    (revision = 0) => revision + 1
  );
}
