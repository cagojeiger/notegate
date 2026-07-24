import { useEffect, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { ApiError } from "../../api/errors";
import { invalidateSpaceResources } from "../../api/queryInvalidation";
import type { RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import type { OpenedNodeRef } from "../../stores/uiStoreReducers";
import { useOpenedNodeQuery } from "./useEditorQueries";

export function OpenedNodeGuard({ nodeRef, children }: { nodeRef: OpenedNodeRef; children: (node: RestNode) => ReactNode }) {
  const nodeQuery = useOpenedNodeQuery(nodeRef);
  const queryClient = useQueryClient();
  const clearGroupsWithNode = useUiStore((state) => state.clearGroupsWithNode);

  useEffect(() => {
    const error = nodeQuery.error;
    if (!(error instanceof ApiError) || error.status !== 404) return;
    clearGroupsWithNode(nodeRef.nodeId);
    invalidateSpaceResources(queryClient, nodeRef.spaceId);
  }, [clearGroupsWithNode, nodeQuery.error, nodeRef.nodeId, nodeRef.spaceId, queryClient]);

  if (nodeQuery.data) return <>{children(nodeQuery.data)}</>;
  if (nodeQuery.isLoading) return <div className="p-10 text-muted">Loading node…</div>;
  return <div className="p-10 text-danger">Could not load node.</div>;
}
