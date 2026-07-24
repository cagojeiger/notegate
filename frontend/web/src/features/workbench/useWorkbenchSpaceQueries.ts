import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { queryKeys } from "../../api/queryKeys";
import { invalidateAuditEvents, removeDeletedSpaceQueries } from "../../api/queryInvalidation";
import { createSpace, deleteSpace, listSpaces, updateSpace } from "../../api/spaces";
import type { SpacesListResponse } from "../../api/types";
import type { Space } from "../../entities/space/model";
import { buildSpaceSortOrderUpdates } from "../spaces/spaceReorder";

export function useSpacesQuery() {
  const client = useApiClient();
  return useQuery({ queryKey: queryKeys.spaces, queryFn: () => listSpaces(client) });
}

export function useCreateSpaceMutation(onCreated: (space: Space) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => createSpace(client, name),
    onSuccess: (space) => {
      queryClient.setQueryData<SpacesListResponse>(queryKeys.spaces, (current) => {
        if (!current) {
          return {
            spaces: [space],
            page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
          };
        }
        if (current.spaces.some((candidate) => candidate.id === space.id)) return current;
        const spaces = [...current.spaces, space].sort((left, right) => left.sort_order - right.sort_order);
        return { ...current, spaces, page: { ...current.page, returned: spaces.length } };
      });
      onCreated(space);
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      invalidateAuditEvents(queryClient);
    }
  });
}

export function useUpdateSpaceMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ spaceId, name, sort_order }: { spaceId: string; name?: string; sort_order?: number }) => updateSpace(client, spaceId, { name, sort_order }),
    onSuccess: () => {
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
      await Promise.all(updates.map((update) => updateSpace(client, update.spaceId, { sort_order: update.sort_order })));
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
    mutationFn: (spaceId: string) => deleteSpace(client, spaceId),
    onSuccess: async (_data, spaceId) => {
      await removeDeletedSpaceQueries(queryClient, spaceId);
      onDeleted(spaceId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      invalidateAuditEvents(queryClient);
    }
  });
}
