import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

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

export function useSpacesQuery() {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client) });
}

export function useCreateSpaceMutation(onCreated: (space: Space) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: (name: string) => createSpace(client, name),
    onSuccess: (space) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      invalidateAuditEvents(queryClient);
      onCreated(space);
    }
  });
}

export function useUpdateSpaceMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
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
