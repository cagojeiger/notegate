import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, ReactNode, useContext, useMemo, useRef } from "react";

import { createApiClient, type ApiClient } from "./client";
import { ApiError } from "./errors";
import { useUiStore } from "../stores/uiStore";

const ApiClientContext = createContext<ApiClient | null>(null);

type ApiProviderProps = {
  apiKey: string | null;
  // Called when any request returns 401 so the app can drop the dead key and
  // send the user back to the login gate instead of silently failing.
  onUnauthorized?: () => void;
  children: ReactNode;
};

export function ApiProvider({ apiKey, onUnauthorized, children }: ApiProviderProps) {
  const client = useMemo(() => createApiClient(() => apiKey), [apiKey]);
  const onUnauthorizedRef = useRef(onUnauthorized);
  onUnauthorizedRef.current = onUnauthorized;

  const queryClientRef = useRef<QueryClient>();
  if (!queryClientRef.current) {
    queryClientRef.current = new QueryClient({
      defaultOptions: {
        queries: {
          // No server push exists, so re-sync external (MCP/REST/other-tab)
          // writes whenever the tab regains focus or the network reconnects.
          refetchOnWindowFocus: true,
          refetchOnReconnect: true,
          staleTime: 5_000,
          retry: 1
        }
      },
      // 401 from any read → bounce to the login gate.
      queryCache: new QueryCache({
        onError: (error) => {
          if (error instanceof ApiError && error.status === 401) onUnauthorizedRef.current?.();
        }
      }),
      // Mutations have no per-call error UI by default, so surface failures:
      // 401 → re-auth, anything else → a toast so writes never fail silently.
      mutationCache: new MutationCache({
        onError: (error) => {
          if (error instanceof ApiError && error.status === 401) {
            onUnauthorizedRef.current?.();
            return;
          }
          const message = error instanceof Error ? error.message : "Request failed";
          useUiStore.getState().showToast(message);
        }
      })
    });
  }

  return (
    <QueryClientProvider client={queryClientRef.current}>
      <ApiClientContext.Provider value={client}>{children}</ApiClientContext.Provider>
    </QueryClientProvider>
  );
}

export function useApiClient(): ApiClient {
  const client = useContext(ApiClientContext);
  if (!client) throw new Error("ApiProvider is missing");
  return client;
}
