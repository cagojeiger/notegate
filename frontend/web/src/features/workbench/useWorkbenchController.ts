import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { getNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { Me, NodeSummary } from "../../api/types";
import { canCreateSpace, canManageSpace, canWriteSpace } from "../../auth/permissions";
import { useIsMobile } from "../../shared/hooks/useMediaQuery";
import { useUiStore } from "../../stores/uiStore";
import { useWorkbenchActions } from "./useWorkbenchActions";
import { useWorkbenchPersistence } from "./useWorkbenchPersistence";
import { useSpacesQuery } from "./useWorkbenchQueries";
import { useSpaceChangeSync } from "./useSpaceChangeSync";

type WorkbenchControllerProps = {
  me: Me;
  onSignOut: () => void;
};

export function useWorkbenchController({ me, onSignOut }: WorkbenchControllerProps) {
  const client = useApiClient();
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
  const activeNode = editorGroups[activeGroupIndex]?.node ?? null;
  const [inspectedNodeId, setInspectedNodeId] = useState<string | null>(activeNode?.id ?? null);
  const inspectNode = useCallback((node: NodeSummary | null) => {
    setInspectedNodeId(node?.id ?? null);
  }, []);
  const activeSpace = useMemo(() => spaces.find((space) => space.id === activeSpaceId) ?? spaces[0] ?? null, [activeSpaceId, spaces]);
  const inspectionTargetId = inspectedNodeId ?? activeNode?.id ?? null;
  const inspectorNodeQuery = useQuery({
    queryKey: activeSpace && inspectionTargetId
      ? queryKeys.node(activeSpace.id, inspectionTargetId)
      : ["node-inspector", "idle"],
    queryFn: () => getNode(client, activeSpace!.id, inspectionTargetId!),
    enabled: Boolean(
      activeSpace
      && inspectionTargetId
      && inspectionTargetId !== activeNode?.id
    ),
    staleTime: Infinity,
    retry: (failureCount, error) => !(error instanceof ApiError && error.status === 404) && failureCount < 3
  });
  const inspectedNode = inspectionTargetId === activeNode?.id
    ? activeNode
    : inspectorNodeQuery.data ?? null;
  const canCreateSpaceForCaller = canCreateSpace(me);
  const canWriteActiveSpace = canWriteSpace(activeSpace);
  const canManageActiveSpace = canManageSpace(me, activeSpace);
  const showAuxiliary = auxiliaryOpen;

  useSpaceChangeSync(activeSpace?.id ?? null);
  useWorkbenchPersistence(theme, activeSpace, activeSpaceId);
  useEffect(() => {
    setInspectedNodeId(activeNode?.id ?? null);
  }, [activeNode?.id]);
  useEffect(() => {
    if (
      inspectedNodeId
      && inspectedNodeId !== activeNode?.id
      && inspectorNodeQuery.error instanceof ApiError
      && inspectorNodeQuery.error.status === 404
    ) {
      setInspectedNodeId(activeNode?.id ?? null);
    }
  }, [activeNode?.id, inspectedNodeId, inspectorNodeQuery.error, setInspectedNodeId]);

  const { settingsOpen, dialog, actions } = useWorkbenchActions({
    activeSpace,
    activeNode,
    inspectedNode,
    canCreateSpace: canCreateSpaceForCaller,
    canWriteActiveSpace,
    canManageActiveSpace,
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
    inspectedNode,
    inspectedNodeId: inspectionTargetId,
    inspectNode,
    inspectorNodeLoading: Boolean(
      inspectionTargetId
      && inspectionTargetId !== activeNode?.id
      && inspectorNodeQuery.isLoading
    ),
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
    settingsOpen,
    dialog,
    actions
  };
}
