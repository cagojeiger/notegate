import type { QueryClient } from "@tanstack/react-query";

import { queryKeys } from "./queryKeys";

export function invalidateAuditEvents(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.auditEvents });
}

export function invalidateSpaceResources(queryClient: QueryClient, spaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.space(spaceId) });
}

export function invalidateSpace(queryClient: QueryClient, spaceId: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.spaces, exact: true });
  invalidateSpaceResources(queryClient, spaceId);
}
