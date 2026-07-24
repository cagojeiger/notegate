import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, ReactNode, useContext, useMemo, useRef } from "react";

import { createApiClient, type ApiClient } from "./client";
import { ApiError } from "./errors";

const ApiClientContext = createContext<ApiClient | null>(null);

type ApiProviderProps = {
  apiKey: string | null;
  authCacheKey: string;
  // Called when any request returns 401 so the app can drop the dead key and
  // send the user back to the login gate instead of silently failing.
  onUnauthorized?: () => void;
  onMutationError?: (message: string) => void;
  children: ReactNode;
};

export function ApiProvider({ apiKey, authCacheKey, onUnauthorized, onMutationError, children }: ApiProviderProps) {
  const client = useMemo(() => createApiClient(() => apiKey), [apiKey]);
  const onUnauthorizedRef = useRef(onUnauthorized);
  const onMutationErrorRef = useRef(onMutationError);
  onUnauthorizedRef.current = onUnauthorized;
  onMutationErrorRef.current = onMutationError;

  const queryClient = useMemo(
    () => {
      // Each authenticated identity gets an isolated query cache.
      void authCacheKey;
      return new QueryClient({
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
        // 401 from resource reads -> bounce to the login gate. The dedicated
        // /me session check handles its own 401 inside AuthBoundary so it can
        // render the login page without a duplicate global reset.
        queryCache: new QueryCache({
          onError: (error, query) => {
            if (error instanceof ApiError && error.status === 401 && query.meta?.authSessionCheck !== true) {
              onUnauthorizedRef.current?.();
            }
          }
        }),
        // Mutations have no per-call error UI by default, so surface failures:
        // 401 -> re-auth, anything else -> a toast so writes never fail silently.
        mutationCache: new MutationCache({
          onError: (error, _variables, _context, mutation) => {
            if (error instanceof ApiError && error.status === 401) {
              onUnauthorizedRef.current?.();
              return;
            }
            // Mutations with their own error UI (e.g. the text-save conflict banner)
            // opt out of the global toast via meta.silentError.
            if (mutation.options.meta?.silentError) return;
            const message = error instanceof Error ? error.message : "Request failed";
            onMutationErrorRef.current?.(message);
          }
        })
      });
    },
    [authCacheKey]
  );

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
