import { useEffect, useMemo, useState } from "react";

import { MAX_EDITOR_GROUPS, WORKBENCH_LAYOUT, type EditorPresentation, type WorkbenchPanelMode } from "../shared/model/workbenchLayout";

export type WorkbenchLayoutInput = {
  isMobile: boolean;
  primaryOpen: boolean;
  auxiliaryOpen: boolean;
  editorGroupCount: number;
  viewportWidth?: number;
};

export type SidebarLayout = {
  availableWidth: number;
  primaryDocked: boolean;
  auxiliaryDocked: boolean;
};

export type WorkbenchLayout = {
  primaryMode: WorkbenchPanelMode;
  auxiliaryMode: WorkbenchPanelMode;
  editorPresentation: EditorPresentation;
  visibleEditorGroupCount: number;
  sidebars?: SidebarLayout;
};

export function resolveWorkbenchLayout(input: WorkbenchLayoutInput): WorkbenchLayout {
  const groupCount = Math.max(1, input.editorGroupCount);

  if (input.isMobile) {
    return {
      primaryMode: input.primaryOpen ? "overlay" : "hidden",
      auxiliaryMode: input.auxiliaryOpen ? "overlay" : "hidden",
      editorPresentation: "focused",
      visibleEditorGroupCount: 1
    };
  }

  const availableWidth = input.viewportWidth === undefined
    ? Infinity
    : Math.max(0, input.viewportWidth - WORKBENCH_LAYOUT.activityRailWidth - WORKBENCH_LAYOUT.minEditorWidth);
  const auxiliaryDocked = input.auxiliaryOpen && (
    !input.primaryOpen || availableWidth >= WORKBENCH_LAYOUT.minPrimaryWidth + WORKBENCH_LAYOUT.minAuxiliaryWidth
  );

  return {
    primaryMode: input.primaryOpen ? "docked" : "hidden",
    auxiliaryMode: !input.auxiliaryOpen ? "hidden" : auxiliaryDocked ? "docked" : "overlay",
    editorPresentation: "split",
    visibleEditorGroupCount: Math.min(groupCount, MAX_EDITOR_GROUPS),
    sidebars: { availableWidth, primaryDocked: input.primaryOpen, auxiliaryDocked }
  };
}

export function resolveSidebarWidths(primaryWidth: number, auxiliaryWidth: number, layout?: SidebarLayout) {
  if (!layout) return { primaryWidth, auxiliaryWidth };
  const { availableWidth, primaryDocked, auxiliaryDocked } = layout;
  const auxiliary = auxiliaryDocked
    ? Math.min(auxiliaryWidth, Math.max(WORKBENCH_LAYOUT.minAuxiliaryWidth, availableWidth - (primaryDocked ? primaryWidth : 0)))
    : 0;
  return {
    primaryWidth: primaryDocked ? Math.min(primaryWidth, availableWidth - auxiliary) : primaryWidth,
    auxiliaryWidth: auxiliaryDocked ? auxiliary : auxiliaryWidth
  };
}

export function useWorkbenchLayout(input: WorkbenchLayoutInput): WorkbenchLayout {
  const { auxiliaryOpen, editorGroupCount, isMobile, primaryOpen } = input;
  const [viewportWidth, setViewportWidth] = useState(() => typeof window === "undefined" ? undefined : window.innerWidth);
  useEffect(() => {
    const onResize = () => setViewportWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return useMemo(
    () => resolveWorkbenchLayout({ auxiliaryOpen, editorGroupCount, isMobile, primaryOpen, viewportWidth }),
    [auxiliaryOpen, editorGroupCount, isMobile, primaryOpen, viewportWidth]
  );
}
