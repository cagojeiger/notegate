import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { queryKeys } from "../../api/queryKeys";
import { createSpace, deleteSpace, listSpaces, updateSpace } from "../../api/spaces";
import type { Space } from "../../api/types";

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
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
      onCreated(space);
    }
  });
}

export function useUpdateSpaceMutation() {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ spaceId, name }: { spaceId: string; name: string }) => updateSpace(client, spaceId, { name }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.spaces })
  });
}

export function useDeleteSpaceMutation(onDeleted: () => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (spaceId: string) => deleteSpace(client, spaceId),
    onSuccess: () => {
      onDeleted();
      void queryClient.invalidateQueries({ queryKey: queryKeys.spaces });
    }
  });
}
