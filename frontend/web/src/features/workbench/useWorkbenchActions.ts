import { useState, type PointerEvent as ReactPointerEvent } from "react";

import type { Space, RestNode } from "../../api/types";
import { clearDevApiKey } from "../../auth/session";
import type { AppDialog } from "../../layout/dialogs/DialogHost";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchNodeActions } from "./useWorkbenchNodeActions";
import { useLogout } from "./useWorkbenchQueries";
import { useWorkbenchSpaceActions } from "./useWorkbenchSpaceActions";

type WorkbenchActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  primaryWidth: number;
  onSignOut: () => void;
};

export function useWorkbenchActions({ activeSpace, activeNode, primaryWidth, onSignOut }: WorkbenchActionsProps) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [dialog, setDialog] = useState<AppDialog | null>(null);

  const addGroup = useUiStore((state) => state.addGroup);
  const closeGroup = useUiStore((state) => state.closeGroup);
  const focusGroup = useUiStore((state) => state.focusGroup);
  const setGroupMode = useUiStore((state) => state.setGroupMode);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const toggleFolder = useUiStore((state) => state.toggleFolder);
  const togglePrimarySidebar = useUiStore((state) => state.togglePrimarySidebar);
  const setPrimaryWidth = useUiStore((state) => state.setPrimaryWidth);
  const toggleAuxiliary = useUiStore((state) => state.toggleAuxiliary);
  const toggleMobileTree = useUiStore((state) => state.toggleMobileTree);
  const toggleMobileAux = useUiStore((state) => state.toggleMobileAux);
  const closeMobile = useUiStore((state) => state.closeMobile);

  const spaceActions = useWorkbenchSpaceActions({ activeSpace, setDialog });
  const nodeActions = useWorkbenchNodeActions({ activeSpace, activeNode, setDialog });
  const logoutSession = useLogout();

  async function handleSignOut() {
    try {
      await logoutSession();
    } finally {
      clearDevApiKey();
      onSignOut();
    }
  }

  function startPrimaryResize(event: ReactPointerEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = primaryWidth;
    const move = (e: PointerEvent) => setPrimaryWidth(startWidth + (e.clientX - startX));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("select-none");
    };
    document.body.classList.add("select-none");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
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
      toggleFolder,
      startPrimaryResize
    }
  };
}
