import type { Dispatch, SetStateAction } from "react";

import type { Space } from "../../api/types";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { deleteSpaceDialog, newSpaceDialog, renameSpaceDialog } from "../../layout/dialogs/appDialogs";
import { useUiStore } from "../../stores/uiStore";
import { useCreateSpaceMutation, useDeleteSpaceMutation, useReorderSpacesMutation, useUpdateSpaceMutation } from "./useWorkbenchQueries";

type SpaceActionsProps = {
  activeSpace: Space | null;
  canCreateSpace: boolean;
  canManageActiveSpace: boolean;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
};

export function useWorkbenchSpaceActions({ activeSpace, canCreateSpace, canManageActiveSpace, setDialog }: SpaceActionsProps) {
  const setActiveSpaceId = useUiStore((state) => state.setActiveSpaceId);
  const resetGroups = useUiStore((state) => state.resetGroups);
  const closeMobile = useUiStore((state) => state.closeMobile);

  const createSpaceMutation = useCreateSpaceMutation((space) => {
    setActiveSpaceId(space.id);
    resetGroups();
  });
  const updateSpaceMutation = useUpdateSpaceMutation();
  const reorderSpacesMutation = useReorderSpacesMutation();
  const deleteSpaceMutation = useDeleteSpaceMutation(() => {
    resetGroups();
    setActiveSpaceId(null);
  });

  function selectSpace(space: Space) {
    setActiveSpaceId(space.id);
    resetGroups();
    closeMobile();
  }

  function promptCreateSpace() {
    if (!canCreateSpace) return;
    setDialog(newSpaceDialog(async (name) => {
      await createSpaceMutation.mutateAsync(name);
    }));
  }

  function reorderSpaces(spaces: Space[]) {
    if (!canCreateSpace) return;
    reorderSpacesMutation.mutate({ spaces });
  }

  function promptRenameSpace(targetSpace = activeSpace) {
    if (!targetSpace || !canManageActiveSpace) return;
    const space = targetSpace;
    setDialog(renameSpaceDialog(space, async (spaceId, name) => {
      await updateSpaceMutation.mutateAsync({ spaceId, name });
    }));
  }

  function confirmDeleteSpace(targetSpace = activeSpace) {
    if (!targetSpace || !canManageActiveSpace) return;
    const space = targetSpace;
    setDialog(deleteSpaceDialog(space, async (spaceId) => {
      await deleteSpaceMutation.mutateAsync(spaceId);
    }));
  }

  return { selectSpace, reorderSpaces, promptCreateSpace, promptRenameSpace, confirmDeleteSpace };
}
