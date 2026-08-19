import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";

import type { NodeLinkProjectionStatus } from "../../api/links";
import { queryKeys } from "../../api/queryKeys";

type LinkProjectionRefreshOptions = {
  spaceId: string;
  nodeId: string;
  status: NodeLinkProjectionStatus | undefined;
};

export function useRefreshNodeLinksAfterProjection({
  spaceId,
  nodeId,
  status
}: LinkProjectionRefreshOptions) {
  const queryClient = useQueryClient();
  const updating = isLinkProjectionActive(status);
  const previous = useRef({ spaceId, nodeId, updating: false });

  useEffect(() => {
    const prior = previous.current;
    const outgoingKey = queryKeys.nodeLinkList(spaceId, nodeId, "outgoing");
    const incomingKey = queryKeys.nodeLinkList(spaceId, nodeId, "incoming");
    const sameNode = prior.spaceId === spaceId && prior.nodeId === nodeId;
    const invalidated = queryClient.getQueryState(outgoingKey)?.isInvalidated === true
      || queryClient.getQueryState(incomingKey)?.isInvalidated === true;

    if (sameNode && status !== undefined && !updating && (prior.updating || invalidated)) {
      void queryClient.resetQueries({ queryKey: outgoingKey, exact: true });
      void queryClient.resetQueries({ queryKey: incomingKey, exact: true });
    }

    previous.current = { spaceId, nodeId, updating };
  }, [nodeId, queryClient, spaceId, status, updating]);
}

export function isLinkProjectionActive(status: NodeLinkProjectionStatus | undefined): boolean {
  return status?.space_pending === true || status?.status === "pending" || status?.status === "syncing";
}
