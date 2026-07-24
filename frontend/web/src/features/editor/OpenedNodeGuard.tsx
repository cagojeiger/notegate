import { useEffect, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { ApiError } from "../../api/errors";
import { invalidateNodeLists, removeDeletedNodeQueries } from "../../api/queryInvalidation";
import type { RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { useNodeFreshness } from "./useEditorQueries";

export function OpenedNodeGuard({ node, children }: { node: RestNode; children: (node: RestNode) => ReactNode }) {
  const freshnessQuery = useNodeFreshness(node);
  const queryClient = useQueryClient();
  const updateGroupsNode = useUiStore((state) => state.updateGroupsNode);
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);
  const latestNode = freshnessQuery.data ?? node;

  useEffect(() => {
    if (freshnessQuery.data) updateGroupsNode(freshnessQuery.data);
  }, [freshnessQuery.data, updateGroupsNode]);

  useEffect(() => {
    const error = freshnessQuery.error;
    if (!(error instanceof ApiError) || error.status !== 404) return;
    clearGroupsWithNode(node.id);
    void removeDeletedNodeQueries(queryClient, node, node.kind === "folder")
      .then(() => invalidateNodeLists(queryClient, node.space_id, [node.parent_id]));
  }, [clearGroupsWithNode, freshnessQuery.error, node, queryClient]);

  return <>{children(latestNode)}</>;
}
