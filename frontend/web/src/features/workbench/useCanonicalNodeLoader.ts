import { useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { getNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { NodeSummary, RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";

export type CanonicalNodeLoader = (
  summary: NodeSummary,
  failureMessage: string
) => Promise<RestNode | null>;

export function useCanonicalNodeLoader(): CanonicalNodeLoader {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);

  return async (summary, failureMessage) => {
    try {
      return await queryClient.fetchQuery({
        queryKey: queryKeys.node(summary.space_id, summary.id),
        queryFn: () => getNode(client, summary.space_id, summary.id),
        staleTime: Number.POSITIVE_INFINITY
      });
    } catch {
      showToast(failureMessage);
      return null;
    }
  };
}
