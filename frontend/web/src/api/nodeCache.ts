import type { InfiniteData, QueryClient } from "@tanstack/react-query";

import type { ChildrenResponse, NodeSummary, RestNode, RestNodeListResponse } from "./types";
import { queryKeys } from "./queryKeys";

type CachedNodeReference = NodeSummary & Partial<RestNode>;

export function updateNodeCaches(
  queryClient: QueryClient,
  node: RestNode,
  update: (current: CachedNodeReference) => CachedNodeReference
) {
  queryClient.setQueryData<RestNode>(
    queryKeys.node(node.space_id, node.id),
    (current) => ({ ...(current ?? node), ...update(current ?? node) })
  );
  updateNodeReferences(queryClient, node.space_id, node.id, update);
}

export function updateExistingNodeCaches(
  queryClient: QueryClient,
  spaceId: string,
  nodeId: string,
  update: (current: CachedNodeReference) => CachedNodeReference
) {
  const canonicalKey = queryKeys.node(spaceId, nodeId);
  if (queryClient.getQueryState(canonicalKey)) {
    queryClient.setQueryData<RestNode>(
      canonicalKey,
      (current) => current ? { ...current, ...update(current) } : current
    );
  }
  updateNodeReferences(queryClient, spaceId, nodeId, update);
}

function updateNodeReferences(
  queryClient: QueryClient,
  spaceId: string,
  nodeId: string,
  update: (current: CachedNodeReference) => CachedNodeReference
) {
  queryClient.setQueryData<InfiniteData<RestNodeListResponse>>(
    queryKeys.recent(spaceId),
    (current) => current ? replaceListPages(current, nodeId, update) : current
  );
  queryClient.setQueriesData<InfiniteData<ChildrenResponse>>(
    {
      predicate: ({ queryKey }) =>
        queryKey.length === 4
        && queryKey[0] === "spaces"
        && queryKey[1] === spaceId
        && queryKey[2] === "children"
    },
    (current) => current ? replaceChildrenNode(current, nodeId, update) : current
  );
}

function replaceListPages(
  current: InfiniteData<RestNodeListResponse>,
  nodeId: string,
  update: (current: CachedNodeReference) => CachedNodeReference
): InfiniteData<RestNodeListResponse> {
  let changed = false;
  const pages = current.pages.map((page) => {
    const nodes = replaceNode(page.nodes, nodeId, update);
    if (nodes === page.nodes) return page;
    changed = true;
    return { ...page, nodes };
  });
  return changed ? { ...current, pages } : current;
}

function replaceChildrenNode(
  current: InfiniteData<ChildrenResponse>,
  nodeId: string,
  update: (current: CachedNodeReference) => CachedNodeReference
): InfiniteData<ChildrenResponse> {
  let changed = false;
  const pages = current.pages.map((page) => {
    const children = replaceNode(page.children, nodeId, update);
    if (children === page.children) return page;
    changed = true;
    return { ...page, children };
  });
  return changed ? { ...current, pages } : current;
}

function replaceNode(
  nodes: NodeSummary[],
  nodeId: string,
  update: (current: CachedNodeReference) => CachedNodeReference
): NodeSummary[] {
  const index = nodes.findIndex((node) => node.id === nodeId);
  if (index < 0) return nodes;
  const current = nodes[index];
  if (!current) return nodes;
  const updated = toNodeSummary(update(current));
  if (sameNodeSummary(updated, current)) return nodes;
  const next = [...nodes];
  next[index] = updated;
  return next;
}

function toNodeSummary(node: CachedNodeReference): NodeSummary {
  return {
    id: node.id,
    space_id: node.space_id,
    parent_id: node.parent_id,
    name: node.name,
    kind: node.kind,
    path: node.path,
    has_children: node.has_children,
    byte_len: node.byte_len,
    line_count: node.line_count,
    preview_available: node.preview_available,
    original_filename: node.original_filename,
    updated_at: node.updated_at
  };
}

function sameNodeSummary(left: NodeSummary, right: NodeSummary): boolean {
  return Object.keys(left).every((key) =>
    left[key as keyof NodeSummary] === right[key as keyof NodeSummary]
  );
}
