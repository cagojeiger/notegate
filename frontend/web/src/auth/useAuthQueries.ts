import { useQuery } from "@tanstack/react-query";

import { useApiClient } from "../api/ApiProvider";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";

export function useSessionQuery(apiKey: string | null) {
  const client = useApiClient();
  return useQuery({ queryKey: [...queryKeys.me, apiKey], queryFn: () => getMe(client), retry: false });
}
