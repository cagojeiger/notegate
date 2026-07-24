import { useEffect } from "react";

import type { Space } from "../../api/types";
import type { ThemeMode } from "../../design/tokens";
import { persistTheme, useUiStore } from "../../stores/uiStore";
import { persistLastActiveSpace, persistSpaceWorkbench } from "../../stores/workbenchStorage";

export function useWorkbenchPersistence(theme: ThemeMode, activeSpace: Space | null, activeSpaceId: string | null) {
  const setActiveSpaceId = useUiStore((state) => state.setActiveSpaceId);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const editorGroups = useUiStore((state) => state.editorGroups);
  const activeGroupIndex = useUiStore((state) => state.activeGroupIndex);

  useEffect(() => {
    persistTheme(theme);
  }, [theme]);

  useEffect(() => {
    if (!activeSpace) return;
    if (activeSpace.id !== activeSpaceId) setActiveSpaceId(activeSpace.id);
    persistLastActiveSpace(activeSpace.id);
    addExpanded([activeSpace.root_node_id]);
  }, [activeSpace, activeSpaceId, setActiveSpaceId, addExpanded]);

  useEffect(() => {
    if (!activeSpace || activeSpace.id !== activeSpaceId) return;
    persistSpaceWorkbench(activeSpace.id, editorGroups, activeGroupIndex);
  }, [activeSpace, activeSpaceId, editorGroups, activeGroupIndex]);
}
