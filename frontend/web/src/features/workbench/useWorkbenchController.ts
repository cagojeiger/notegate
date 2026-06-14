import { useMemo } from "react";

import { useIsMobile } from "../../shared/hooks/useMediaQuery";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchActions } from "./useWorkbenchActions";
import { useWorkbenchPersistence } from "./useWorkbenchPersistence";
import { useSpacesQuery } from "./useWorkbenchQueries";

type WorkbenchControllerProps = {
  onSignOut: () => void;
};

export function useWorkbenchController({ onSignOut }: WorkbenchControllerProps) {
  const spacesQuery = useSpacesQuery();
  const spaces = spacesQuery.data?.spaces ?? [];

  const theme = useUiStore((state) => state.theme);
  const activeSpaceId = useUiStore((state) => state.activeSpaceId);
  const editorGroups = useUiStore((state) => state.editorGroups);
  const activeGroupIndex = useUiStore((state) => state.activeGroupIndex);
  const expandedFolderIds = useUiStore((state) => state.expandedFolderIds);
  const primarySidebarOpen = useUiStore((state) => state.primarySidebarOpen);
  const auxiliaryOpen = useUiStore((state) => state.auxiliaryOpen);
  const primaryWidth = useUiStore((state) => state.primaryWidth);
  const mobileTreeOpen = useUiStore((state) => state.mobileTreeOpen);
  const mobileAuxOpen = useUiStore((state) => state.mobileAuxOpen);
  const isMobile = useIsMobile();
  const activeNode = editorGroups[activeGroupIndex]?.node ?? null;
  const activeSpace = useMemo(() => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0] ?? null, [activeSpaceId, spaces]);
  const showAuxiliary = auxiliaryOpen;

  useWorkbenchPersistence(theme, activeSpace);

  const { settingsOpen, dialog, actions } = useWorkbenchActions({
    activeSpace,
    activeNode,
    primaryWidth,
    onSignOut
  });

  return {
    loading: spacesQuery.isLoading,
    error: spacesQuery.isError ? String(spacesQuery.error) : null,
    spaces,
    theme,
    activeSpace,
    activeNode,
    editorGroups,
    activeGroupIndex,
    expandedFolderIds,
    primarySidebarOpen,
    auxiliaryOpen,
    primaryWidth,
    mobileTreeOpen,
    mobileAuxOpen,
    showAuxiliary,
    isMobile,
    settingsOpen,
    dialog,
    actions
  };
}
