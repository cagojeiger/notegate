import { useEffect } from "react";

import type { Space } from "../../api/types";
import type { ThemeMode } from "../../design/tokens";
import { persistLastSpace, persistTheme, useUiStore } from "../../stores/uiStore";

export function useWorkbenchPersistence(theme: ThemeMode, activeSpace: Space | null) {
  const setActiveSpaceId = useUiStore((state) => state.setActiveSpaceId);
  const addExpanded = useUiStore((state) => state.addExpanded);

  useEffect(() => {
    persistTheme(theme);
  }, [theme]);

  useEffect(() => {
    if (!activeSpace) return;
    setActiveSpaceId(activeSpace.id);
    persistLastSpace(activeSpace.id);
    addExpanded([activeSpace.root_node_id]);
  }, [activeSpace, setActiveSpaceId, addExpanded]);
}
