import { useQueryClient } from "@tanstack/react-query";

import { invalidateSpace } from "../../api/queryInvalidation";

export function useInvalidateSpace() {
  const queryClient = useQueryClient();
  return (spaceId: string) => invalidateSpace(queryClient, spaceId);
}
