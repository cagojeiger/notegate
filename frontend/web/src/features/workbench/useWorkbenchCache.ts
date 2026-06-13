import { useQueryClient } from "@tanstack/react-query";

import { queryKeys } from "../../api/queryKeys";

export function useInvalidateSpace() {
  const queryClient = useQueryClient();
  return (spaceId: string) => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
    void queryClient.invalidateQueries({ queryKey: ["spaces", spaceId] });
  };
}
