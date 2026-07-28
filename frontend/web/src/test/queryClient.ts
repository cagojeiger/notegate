import { QueryClient, type QueryClientConfig } from "@tanstack/react-query";

export function createTestQueryClient(config: QueryClientConfig = {}): QueryClient {
  return new QueryClient({
    ...config,
    defaultOptions: {
      ...config.defaultOptions,
      queries: {
        retry: false,
        ...config.defaultOptions?.queries
      },
      mutations: {
        retry: false,
        ...config.defaultOptions?.mutations
      }
    }
  });
}
