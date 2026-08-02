import { useState } from "react";

import type { Space, RestNode } from "../../api/types";
import type { AppDialog } from "./dialogs/dialogTypes";
import { useUiStore } from "../../stores/uiStore";
import { clearPersistedWorkbenches } from "../../stores/workbenchStorage";
import { useWorkbenchNodeActions } from "./useWorkbenchNodeActions";
import { useLogout } from "./useWorkbenchQueries";
import { useWorkbenchSpaceActions } from "./useWorkbenchSpaceActions";

type WorkbenchActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  inspectedNode: RestNode | null;
  canCreateSpace: boolean;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  onSignOut: () => void;
};

export function useWorkbenchActions({ activeSpace, activeNode, inspectedNode, canCreateSpace, canWriteActiveSpace, canManageActiveSpace, onSignOut }: WorkbenchActionsProps) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [dialog, setDialog] = useState<AppDialog | null>(null);

  const addGroup = useUiStore((state) => state.addGroup);
  const closeGroup = useUiStore((state) => state.closeGroup);
  const focusGroup = useUiStore((state) => state.focusGroup);
  const setGroupMode = useUiStore((state) => state.setGroupMode);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const toggleFolder = useUiStore((state) => state.toggleFolder);
  const togglePrimarySidebar = useUiStore((state) => state.togglePrimarySidebar);
  const toggleAuxiliary = useUiStore((state) => state.toggleAuxiliary);
  const toggleMobileTree = useUiStore((state) => state.toggleMobileTree);
  const toggleMobileAux = useUiStore((state) => state.toggleMobileAux);
  const closeMobile = useUiStore((state) => state.closeMobile);
  const showToast = useUiStore((state) => state.showToast);

  const spaceActions = useWorkbenchSpaceActions({ activeSpace, canCreateSpace, setDialog });
  const nodeActions = useWorkbenchNodeActions({
    activeSpace,
    activeNode,
    inspectedNode,
    canWriteActiveSpace,
    canManageActiveSpace,
    setDialog
  });
  const logoutSession = useLogout();

  async function handleSignOut() {
    try {
      await logoutSession();
    } finally {
      clearPersistedWorkbenches();
      onSignOut();
    }
  }

  function confirmResetSavedWorkspace() {
    setDialog({
      kind: "confirm",
      title: "Reset saved workspace",
      message: "This clears saved open panes and panel visibility for this browser only. Your notes and spaces will not be deleted.",
      confirmLabel: "Reset",
      danger: true,
      onConfirm: () => {
        clearPersistedWorkbenches();
        showToast("Saved workspace reset");
      }
    });
  }

  return {
    settingsOpen,
    dialog,
    actions: {
      addGroup,
      closeGroup,
      focusGroup,
      setGroupMode,
      toggleTheme,
      togglePrimarySidebar,
      toggleAuxiliary,
      toggleMobileTree,
      toggleMobileAux,
      closeMobile,
      setSettingsOpen,
      setDialog,
      ...spaceActions,
      ...nodeActions,
      handleSignOut,
      confirmResetSavedWorkspace,
      toggleFolder
    }
  };
}
