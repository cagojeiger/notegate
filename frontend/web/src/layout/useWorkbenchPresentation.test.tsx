import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { RestNode } from "../api/types";
import { useWorkbenchPresentation } from "./useWorkbenchPresentation";

const pdfNode: RestNode = {
  id: "pdf-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "document.pdf",
  kind: "file",
  path: "/document.pdf",
  sort_order: 0,
  metadata: {},
  has_children: false,
  detected_media_type: "application/pdf",
  preview_available: true,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-07-24T00:00:00Z",
  updated_at: "2026-07-24T00:00:00Z"
};

function input(overrides: Partial<Parameters<typeof useWorkbenchPresentation>[0]> = {}) {
  return {
    activeNode: pdfNode,
    auxiliaryOpen: true,
    editorGroupCount: 1,
    isMobile: false,
    mobileAuxiliaryOpen: false,
    mobilePrimaryOpen: false,
    onToggleAuxiliary: vi.fn(),
    onToggleMobileAuxiliary: vi.fn(),
    onToggleMobilePrimary: vi.fn(),
    onTogglePrimary: vi.fn(),
    primaryOpen: true,
    primaryWidth: 300,
    viewportWidth: 1024,
    ...overrides
  };
}

describe("useWorkbenchPresentation", () => {
  it("folds the desktop Inspector for a verified PDF and restores it as a local override", () => {
    const props = input();
    const { result } = renderHook(() => useWorkbenchPresentation(props));

    expect(result.current.primaryOpen).toBe(true);
    expect(result.current.auxiliaryOpen).toBe(false);
    expect(result.current.layout.auxiliaryMode).toBe("hidden");

    act(() => result.current.toggleAuxiliary());

    expect(result.current.auxiliaryOpen).toBe(true);
    expect(props.onToggleAuxiliary).not.toHaveBeenCalled();
  });

  it("focuses the active editor and folds only the Inspector at the 900px tablet size", () => {
    const { result } = renderHook(() => useWorkbenchPresentation(input({ editorGroupCount: 2, viewportWidth: 900 })));

    expect(result.current.primaryOpen).toBe(true);
    expect(result.current.auxiliaryOpen).toBe(false);
    expect(result.current.layout.editorPresentation).toBe("focused");
    expect(result.current.layout.visibleEditorGroupCount).toBe(1);
  });

  it("uses mobile overlay state and actions instead of desktop reading overrides", () => {
    const props = input({
      isMobile: true,
      mobilePrimaryOpen: true,
      mobileAuxiliaryOpen: false
    });
    const { result } = renderHook(() => useWorkbenchPresentation(props));

    expect(result.current.layout.primaryMode).toBe("overlay");
    expect(result.current.layout.auxiliaryMode).toBe("hidden");
    expect(result.current.mobileOverlayVisible).toBe(true);

    act(() => result.current.togglePrimary());

    expect(props.onToggleMobilePrimary).toHaveBeenCalledOnce();
    expect(props.onTogglePrimary).not.toHaveBeenCalled();
  });

  it("uses normal panel behavior when a declared PDF was not verified", () => {
    const props = input({
      activeNode: {
        ...pdfNode,
        detected_media_type: undefined,
        preview_available: false
      }
    });
    const { result } = renderHook(() => useWorkbenchPresentation(props));

    expect(result.current.auxiliaryOpen).toBe(true);
    act(() => result.current.toggleAuxiliary());
    expect(props.onToggleAuxiliary).toHaveBeenCalledOnce();
  });
});
