import { useQuery } from "@tanstack/react-query";

import { useApiClient } from "../api/ApiProvider";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";

export function useSessionQuery(apiKey: string | null, sessionRevision: number) {
  const client = useApiClient();
  return useQuery({
    queryKey: [...queryKeys.me, apiKey, sessionRevision],
    queryFn: () => getMe(client),
    retry: false,
    meta: { authSessionCheck: true }
  });
}
