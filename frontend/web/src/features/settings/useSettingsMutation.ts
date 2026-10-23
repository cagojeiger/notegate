import { useMutation, useMutationState, useQueryClient, type MutationState, type QueryKey, type UseMutationOptions } from "@tanstack/react-query";

import { useIsCurrentApiCache } from "../../api/ApiProvider";

type Operation = "trash" | "delete-agent" | "revoke-browser" | "revoke-cli" | "revoke-mcp" | "revoke-api-key";

// Non-secret settings operations outlive their view in the authenticated query cache.
export function useSettingsMutation<TData, TVariables>(operation: Operation, options: UseMutationOptions<TData, Error, TVariables>, scope?: QueryKey) {
  const client = useQueryClient();
  const isCurrentCache = useIsCurrentApiCache();
  const cache = client.getMutationCache();
  const mutationKey = scope ? ["settings-operation", operation, scope] as const : ["settings-operation", operation] as const;
  const filters = { mutationKey, exact: true };
  const mutation = useMutation({
    ...options, mutationKey,
    onSuccess: (...args) => { if (isCurrentCache()) return options.onSuccess?.(...args); },
    onError: (...args) => { if (isCurrentCache()) return options.onError?.(...args); },
    onSettled: (...args) => { if (isCurrentCache()) return options.onSettled?.(...args); }
  });
  const states = useMutationState<MutationState<TData, Error, TVariables>>({
    filters, select: (item) => item.state
  });
  const state = states[states.length - 1];
  const pending = () => cache.find({ ...filters, status: "pending" }) !== undefined;
  const clear = () => { cache.findAll(filters).forEach((item) => cache.remove(item)); };

  return {
    isPending: state?.status === "pending",
    isSuccess: state?.status === "success",
    isError: state?.status === "error",
    error: state?.error ?? null,
    data: state?.data,
    variables: state?.variables,
    mutate: (variables: TVariables) => {
      // Check the cache synchronously, including calls from a stale or remounted view.
      if (!isCurrentCache() || pending()) return;
      clear();
      mutation.mutate(variables);
    },
    reset: () => {
      if (pending()) return;
      mutation.reset();
      clear();
    }
  };
}
