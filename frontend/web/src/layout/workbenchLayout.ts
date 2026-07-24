import { useMemo } from "react";

import { MAX_EDITOR_GROUPS } from "../stores/uiStoreReducers";

export type WorkbenchPanelMode = "hidden" | "overlay" | "docked";
export type EditorPresentation = "split" | "focused";

export const WORKBENCH_LAYOUT = {
  defaultPrimaryWidth: 300,
  minPrimaryWidth: 220,
  maxPrimaryWidth: 520,
  auxiliaryWidth: 320,
  mobilePrimaryWidthPercent: "85%",
  mobilePrimaryMaxWidth: 320,
  defaultTreeRatio: 0.67,
  minTreeRatio: 0.2,
  maxTreeRatio: 0.82
} as const;

export const PDF_MIN_READING_WIDTH = 480;

export type WorkbenchLayoutInput = {
  isMobile: boolean;
  primaryOpen: boolean;
  auxiliaryOpen: boolean;
  editorGroupCount: number;
  focusEditor?: boolean;
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
    editorPresentation: input.focusEditor ? "focused" : "split",
    visibleEditorGroupCount: input.focusEditor ? 1 : Math.min(groupCount, MAX_EDITOR_GROUPS)
  };
}

export function useWorkbenchLayout(input: WorkbenchLayoutInput): WorkbenchLayout {
  const { auxiliaryOpen, editorGroupCount, focusEditor, isMobile, primaryOpen } = input;
  return useMemo(
    () => resolveWorkbenchLayout({ auxiliaryOpen, editorGroupCount, focusEditor, isMobile, primaryOpen }),
    [auxiliaryOpen, editorGroupCount, focusEditor, isMobile, primaryOpen]
  );
}

export function resolvePdfReadingLayout({
  auxiliaryOpen,
  editorGroupCount,
  primaryOpen,
  primaryWidth,
  viewportWidth
}: {
  auxiliaryOpen: boolean;
  editorGroupCount: number;
  primaryOpen: boolean;
  primaryWidth: number;
  viewportWidth: number;
}): { foldAuxiliary: boolean; foldPrimary: boolean; focusEditor: boolean } {
  if (viewportWidth < 768) {
    return { foldAuxiliary: false, foldPrimary: false, focusEditor: false };
  }

  const groupCount = Math.max(1, Math.min(editorGroupCount, MAX_EDITOR_GROUPS));
  const pagePadding = viewportWidth >= 1024 ? 80 : 32;
  let foldAuxiliary = false;
  let foldPrimary = false;
  let focusEditor = false;
  const contentWidth = () => {
    const primary = primaryOpen && !foldPrimary ? primaryWidth + 4 : 0;
    const auxiliary = auxiliaryOpen && !foldAuxiliary ? WORKBENCH_LAYOUT.auxiliaryWidth : 0;
    const editor = Math.max(0, viewportWidth - 52 - primary - auxiliary);
    return editor / (focusEditor ? 1 : groupCount) - pagePadding;
  };

  if (groupCount > 1 && contentWidth() < PDF_MIN_READING_WIDTH) focusEditor = true;
  if (auxiliaryOpen && contentWidth() < PDF_MIN_READING_WIDTH) foldAuxiliary = true;
  if (primaryOpen && contentWidth() < PDF_MIN_READING_WIDTH) foldPrimary = true;
  return { foldAuxiliary, foldPrimary, focusEditor };
}
