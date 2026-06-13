import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, ReactNode, useContext, useMemo } from "react";

import { createApiClient, type ApiClient } from "./client";

const ApiClientContext = createContext<ApiClient | null>(null);

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // External writes (MCP, REST, another tab) have no server push to rely on,
      // so re-sync whenever the tab regains focus or the network reconnects.
      refetchOnWindowFocus: true,
      refetchOnReconnect: true,
      staleTime: 5_000,
      retry: 1
    }
  }
});

type ApiProviderProps = {
  apiKey: string | null;
  children: ReactNode;
};

export function ApiProvider({ apiKey, children }: ApiProviderProps) {
  const client = useMemo(() => createApiClient(() => apiKey), [apiKey]);

  return (
    <QueryClientProvider client={queryClient}>
      <ApiClientContext.Provider value={client}>{children}</ApiClientContext.Provider>
    </QueryClientProvider>
  );
}

export function useApiClient(): ApiClient {
  const client = useContext(ApiClientContext);
  if (!client) throw new Error("ApiProvider is missing");
  return client;
}
