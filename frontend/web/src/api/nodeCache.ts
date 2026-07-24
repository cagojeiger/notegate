import type { InfiniteData, QueryClient } from "@tanstack/react-query";

import type { ChildrenResponse, RestNode, RestNodeListResponse } from "./types";
import { queryKeys } from "./queryKeys";

export function updateNodeCaches(
  queryClient: QueryClient,
  node: RestNode,
  update: (current: RestNode) => RestNode
) {
  const updateNode = (current: RestNode | undefined) => update(current ?? node);
  queryClient.setQueryData<RestNode>(queryKeys.node(node.space_id, node.id), updateNode);
  const pathQueryKey = queryKeys.markdownImageNode(node.space_id, node.path);
  if (queryClient.getQueryState(pathQueryKey)) {
    queryClient.setQueryData<RestNode>(pathQueryKey, updateNode);
  }
  queryClient.setQueryData<RestNodeListResponse>(
    queryKeys.recent(node.space_id),
    (current) => current ? replaceListNode(current, node.id, update) : current
  );
  queryClient.setQueriesData<InfiniteData<ChildrenResponse>>(
    {
      predicate: ({ queryKey }) =>
        queryKey.length === 4
        && queryKey[0] === "spaces"
        && queryKey[1] === node.space_id
        && queryKey[2] === "children"
    },
    (current) => current ? replaceChildrenNode(current, node.id, update) : current
  );
}

function replaceListNode(
  current: RestNodeListResponse,
  nodeId: string,
  update: (current: RestNode) => RestNode
): RestNodeListResponse {
  const nodes = replaceNode(current.nodes, nodeId, update);
  return nodes === current.nodes ? current : { ...current, nodes };
}

function replaceChildrenNode(
  current: InfiniteData<ChildrenResponse>,
  nodeId: string,
  update: (current: RestNode) => RestNode
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
  nodes: RestNode[],
  nodeId: string,
  update: (current: RestNode) => RestNode
): RestNode[] {
  const index = nodes.findIndex((node) => node.id === nodeId);
  if (index < 0) return nodes;
  const current = nodes[index];
  if (!current) return nodes;
  const updated = update(current);
  if (updated === current) return nodes;
  const next = [...nodes];
  next[index] = updated;
  return next;
}
