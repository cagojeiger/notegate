import type { QueryClient } from "@tanstack/react-query";

import { queryKeys } from "./queryKeys";

export function advanceChildrenRevision(queryClient: QueryClient, spaceId: string) {
  queryClient.setQueryData<number>(
    queryKeys.childrenRevision(spaceId),
    (revision = 0) => revision + 1
  );
}
