import { useEffect } from "react";

import type { Space } from "../../api/types";
import type { ThemeMode } from "../../design/tokens";
import { useUiStore } from "../../stores/uiStore";
import {
  browserUiStorePersistence,
  type UiStorePersistence
} from "../../stores/uiStorePersistence";

export type WorkbenchPersistence = Pick<
  UiStorePersistence,
  "saveTheme" | "saveLastActiveSpaceId" | "saveSpaceWorkbench"
>;

export function useWorkbenchPersistence(
  theme: ThemeMode,
  activeSpace: Space | null,
  activeSpaceId: string | null,
  persistence: WorkbenchPersistence = browserUiStorePersistence
) {
  const setActiveSpaceId = useUiStore((state) => state.setActiveSpaceId);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const editorGroups = useUiStore((state) => state.editorGroups);
  const activeGroupIndex = useUiStore((state) => state.activeGroupIndex);

  useEffect(() => {
    persistence.saveTheme(theme);
  }, [persistence, theme]);

  useEffect(() => {
    if (!activeSpace) return;
    if (activeSpace.id !== activeSpaceId) setActiveSpaceId(activeSpace.id);
    persistence.saveLastActiveSpaceId(activeSpace.id);
    addExpanded([activeSpace.root_node_id]);
  }, [activeSpace, activeSpaceId, persistence, setActiveSpaceId, addExpanded]);

  useEffect(() => {
    if (!activeSpace || activeSpace.id !== activeSpaceId) return;
    persistence.saveSpaceWorkbench(activeSpace.id, editorGroups, activeGroupIndex);
  }, [activeSpace, activeSpaceId, editorGroups, activeGroupIndex, persistence]);
}
