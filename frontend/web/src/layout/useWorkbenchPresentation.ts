import { useState } from "react";

import type { RestNode } from "../api/types";
import { filePreviewKind } from "../shared/lib/filePreview";
import { resolvePdfReadingLayout, useWorkbenchLayout } from "./workbenchLayout";

type WorkbenchPresentationInput = {
  activeNode: RestNode | null;
  auxiliaryOpen: boolean;
  editorGroupCount: number;
  isMobile: boolean;
  mobileAuxiliaryOpen: boolean;
  mobilePrimaryOpen: boolean;
  onToggleAuxiliary: () => void;
  onToggleMobileAuxiliary: () => void;
  onToggleMobilePrimary: () => void;
  onTogglePrimary: () => void;
  primaryOpen: boolean;
  primaryWidth: number;
  viewportWidth: number;
};

type ReadingPanelOverrides = {
  nodeId: string;
  auxiliaryOpen?: boolean;
  primaryOpen?: boolean;
};

export function useWorkbenchPresentation(input: WorkbenchPresentationInput) {
  const [readingPanelOverrides, setReadingPanelOverrides] = useState<ReadingPanelOverrides | null>(null);
  const activeReadingNodeId = isVerifiedPdf(input.activeNode) ? input.activeNode.id : null;
  const readingLayout = activeReadingNodeId && !input.isMobile
    ? resolvePdfReadingLayout({
        auxiliaryOpen: input.auxiliaryOpen,
        editorGroupCount: input.editorGroupCount,
        primaryOpen: input.primaryOpen,
        primaryWidth: input.primaryWidth,
        viewportWidth: input.viewportWidth
      })
    : { foldAuxiliary: false, foldPrimary: false, focusEditor: false };
  const activeOverrides = readingPanelOverrides?.nodeId === activeReadingNodeId ? readingPanelOverrides : null;
  const primaryOpen = input.isMobile
    ? input.mobilePrimaryOpen
    : activeOverrides?.primaryOpen
      ?? (readingLayout.foldPrimary ? false : input.primaryOpen);
  const auxiliaryOpen = input.isMobile
    ? input.mobileAuxiliaryOpen
    : activeOverrides?.auxiliaryOpen
      ?? (readingLayout.foldAuxiliary ? false : input.auxiliaryOpen);
  const layout = useWorkbenchLayout({
    isMobile: input.isMobile,
    primaryOpen,
    auxiliaryOpen,
    editorGroupCount: input.editorGroupCount,
    focusEditor: readingLayout.focusEditor
  });

  function updateReadingPanelOverrides(update: Omit<ReadingPanelOverrides, "nodeId">) {
    if (!activeReadingNodeId) return;
    setReadingPanelOverrides((current) => ({
      ...(current?.nodeId === activeReadingNodeId ? current : {}),
      nodeId: activeReadingNodeId,
      ...update
    }));
  }

  function togglePrimary() {
    if (input.isMobile) {
      input.onToggleMobilePrimary();
      return;
    }
    if (activeReadingNodeId && (readingLayout.foldPrimary || activeOverrides?.primaryOpen !== undefined)) {
      updateReadingPanelOverrides({ primaryOpen: !primaryOpen });
      return;
    }
    input.onTogglePrimary();
  }

  function toggleAuxiliary() {
    if (input.isMobile) {
      input.onToggleMobileAuxiliary();
      return;
    }
    if (activeReadingNodeId && (readingLayout.foldAuxiliary || activeOverrides?.auxiliaryOpen !== undefined)) {
      updateReadingPanelOverrides({ auxiliaryOpen: !auxiliaryOpen });
      return;
    }
    input.onToggleAuxiliary();
  }

  return {
    auxiliaryOpen,
    layout,
    mobileOverlayVisible: input.isMobile && (layout.primaryMode === "overlay" || layout.auxiliaryMode === "overlay"),
    primaryOpen,
    toggleAuxiliary,
    togglePrimary
  };
}

function isVerifiedPdf(node: RestNode | null): node is RestNode {
  return node?.kind === "file"
    && node.preview_available === true
    && filePreviewKind(node.detected_media_type) === "pdf";
}
