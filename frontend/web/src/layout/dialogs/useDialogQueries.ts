import { useQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";

export function useMovePickerChildren(spaceId: string, nodeId: string) {
  const client = useApiClient();
  return useQuery({
    queryKey: [...queryKeys.children(spaceId, nodeId), "move-picker"],
    queryFn: () => listChildren(client, spaceId, nodeId)
  });
}
