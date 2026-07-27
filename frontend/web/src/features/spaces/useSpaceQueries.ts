import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseMutationOptions
} from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { invalidateAuditEvents, removeDeletedSpaceQueries } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import {
  createSpace,
  deleteSpace,
  listSpaces,
  reorderSpaces as reorderSpacesRequest,
  updateSpace
} from "../../api/spaces";
import type { UpdateSpaceInput } from "../../api/spaces";
import type { Space, SpacesListResponse } from "../../api/types";
import { buildSpaceSortOrderUpdates } from "./spaceReorder";

type SpaceMutationOptions = {
  silentError?: boolean;
};

export function useSpacesQuery() {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client) });
}

export function createSpaceMutationOptions(
  createRequest: (name: string) => Promise<Space>,
  queryClient: QueryClient,
  onCreated: (space: Space) => void
): UseMutationOptions<Space, Error, string> {
  return {
    meta: { silentError: true },
    mutationFn: createRequest,
    onSuccess: (space) => {
      queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, (current) => {
        if (!current) {
          return {
            spaces: [space],
            page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
          };
        }
        const spaces = [
          ...current.spaces.filter((candidate) => candidate.id !== space.id),
          space
        ].sort((left, right) => left.sort_order - right.sort_order);
        return { ...current, spaces, page: { ...current.page, returned: spaces.length } };
      });
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      invalidateAuditEvents(queryClient);
      onCreated(space);
    }
  };
}

export function useCreateSpaceMutation(onCreated: (space: Space) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation(
    createSpaceMutationOptions(
      (name) => createSpace(client, name),
      queryClient,
      onCreated
    )
  );
}

export function useUpdateSpaceMutation(options: SpaceMutationOptions = {}) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: options.silentError ? { silentError: true } : undefined,
    mutationFn: ({ spaceId, ...input }: UpdateSpaceInput & { spaceId: string }) =>
      updateSpace(client, spaceId, input),
    onSuccess: (updatedSpace) => {
      queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, (current) => current ? {
        ...current,
        spaces: current.spaces.map((space) => space.id === updatedSpace.id ? updatedSpace : space)
      } : current);
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      invalidateAuditEvents(queryClient);
    }
  });
}

export function useReorderSpacesMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ spaces }: { spaces: Space[] }) => {
      const updates = buildSpaceSortOrderUpdates(spaces);
      if (updates.length > 0) await reorderSpacesRequest(client, updates);
    },
    onMutate: async ({ spaces }) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.spaces });
      const previous = queryClient.getQueryData<SpacesListResponse>(queryKeys.spaces);
      if (previous) queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, { ...previous, spaces });
      return { previous };
    },
    onError: (_error, _variables, context) => {
      if (context?.previous) queryClient.setQueryData(queryKeys.spaces, context.previous);
    },
    onSettled: (_data, error) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      if (!error) invalidateAuditEvents(queryClient);
    }
  });
}

export function useDeleteSpaceMutation(onDeleted: (spaceId: string) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: (spaceId: string) => deleteSpace(client, spaceId),
    onSuccess: async (_data, spaceId) => {
      await removeDeletedSpaceQueries(queryClient, spaceId);
      onDeleted(spaceId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      invalidateAuditEvents(queryClient);
    }
  });
}
