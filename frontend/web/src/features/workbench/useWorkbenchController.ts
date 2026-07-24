import { useMemo } from "react";

import type { Me } from "../../api/types";
import { canCreateSpace, canManageSpace, canWriteSpace } from "../../auth/permissions";
import { useIsMobile, useIsTablet } from "../../shared/hooks/useMediaQuery";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchActions } from "./useWorkbenchActions";
import { useWorkbenchPersistence } from "./useWorkbenchPersistence";
import { useSpacesQuery } from "./useWorkbenchQueries";

type WorkbenchControllerProps = {
  me: Me;
  onSignOut: () => void;
};

export function useWorkbenchController({ me, onSignOut }: WorkbenchControllerProps) {
  const spacesQuery = useSpacesQuery();
  const spaces = useMemo(() => spacesQuery.data?.spaces ?? [], [spacesQuery.data?.spaces]);

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
  const isTablet = useIsTablet();
  const activeNode = editorGroups[activeGroupIndex]?.node ?? null;
  const activeSpace = useMemo(() => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0] ?? null, [activeSpaceId, spaces]);
  const canCreateSpaceForCaller = canCreateSpace(me);
  const canWriteActiveSpace = canWriteSpace(activeSpace);
  const canManageActiveSpace = canManageSpace(me, activeSpace);
  const showAuxiliary = auxiliaryOpen;

  useWorkbenchPersistence(theme, activeSpace, activeSpaceId);

  const { settingsOpen, dialog, actions } = useWorkbenchActions({
    activeSpace,
    activeNode,
    canCreateSpace: canCreateSpaceForCaller,
    canWriteActiveSpace,
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
    canCreateSpace: canCreateSpaceForCaller,
    canWriteActiveSpace,
    canManageActiveSpace,
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
    isTablet,
    settingsOpen,
    dialog,
    actions
  };
}
