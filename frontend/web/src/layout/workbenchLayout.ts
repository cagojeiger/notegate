import { useMemo } from "react";

import { MAX_EDITOR_GROUPS, type EditorPresentation, type WorkbenchPanelMode } from "../shared/model/workbenchLayout";

export type WorkbenchLayoutInput = {
  isMobile: boolean;
  primaryOpen: boolean;
  auxiliaryOpen: boolean;
  editorGroupCount: number;
};

export type WorkbenchLayout = {
  primaryMode: WorkbenchPanelMode;
  auxiliaryMode: WorkbenchPanelMode;
  editorPresentation: EditorPresentation;
  visibleEditorGroupCount: number;
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

  return {
    primaryMode: input.primaryOpen ? "docked" : "hidden",
    auxiliaryMode: input.auxiliaryOpen ? "docked" : "hidden",
    editorPresentation: "split",
    visibleEditorGroupCount: Math.min(groupCount, MAX_EDITOR_GROUPS)
  };
}

export function useWorkbenchLayout(input: WorkbenchLayoutInput): WorkbenchLayout {
  const { auxiliaryOpen, editorGroupCount, isMobile, primaryOpen } = input;
  return useMemo(
    () => resolveWorkbenchLayout({ auxiliaryOpen, editorGroupCount, isMobile, primaryOpen }),
    [auxiliaryOpen, editorGroupCount, isMobile, primaryOpen]
  );
}
