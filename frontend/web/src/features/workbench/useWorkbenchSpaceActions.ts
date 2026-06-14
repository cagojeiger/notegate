import type { Dispatch, SetStateAction } from "react";

import type { Space } from "../../api/types";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { deleteSpaceDialog, newSpaceDialog, renameSpaceDialog } from "../../layout/dialogs/appDialogs";
import { useUiStore } from "../../stores/uiStore";
import { useCreateSpaceMutation, useDeleteSpaceMutation, useReorderSpacesMutation, useUpdateSpaceMutation } from "./useWorkbenchQueries";

type SpaceActionsProps = {
  activeSpace: Space | null;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
};

export function useWorkbenchSpaceActions({ activeSpace, setDialog }: SpaceActionsProps) {
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
    setDialog(newSpaceDialog(async (name) => {
      await createSpaceMutation.mutateAsync(name);
    }));
  }

  function reorderSpaces(spaces: Space[]) {
    reorderSpacesMutation.mutate({ spaces });
  }

  function promptRenameSpace() {
    if (!activeSpace) return;
    const space = activeSpace;
    setDialog(renameSpaceDialog(space, async (spaceId, name) => {
      await updateSpaceMutation.mutateAsync({ spaceId, name });
    }));
  }

  function confirmDeleteSpace() {
    if (!activeSpace) return;
    const space = activeSpace;
    setDialog(deleteSpaceDialog(space, async (spaceId) => {
      await deleteSpaceMutation.mutateAsync(spaceId);
    }));
  }

  return { selectSpace, reorderSpaces, promptCreateSpace, promptRenameSpace, confirmDeleteSpace };
}
