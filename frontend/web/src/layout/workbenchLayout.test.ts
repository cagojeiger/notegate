import { describe, expect, it } from "vitest";

import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";
import { resolveWorkbenchLayout } from "./workbenchLayout";

const base = {
  isMobile: false,
  primaryOpen: true,
  auxiliaryOpen: true,
  editorGroupCount: 1
};

describe("resolveWorkbenchLayout", () => {
  it("docks both side panels and shows one editor pane when one group is open", () => {
    expect(resolveWorkbenchLayout(base)).toMatchObject({
      primaryMode: "docked",
      auxiliaryMode: "docked",
      editorPresentation: "split",
      visibleEditorGroupCount: 1
    });
  });

  it("keeps two non-mobile panes when two groups are open", () => {
    expect(resolveWorkbenchLayout({ ...base, editorGroupCount: 2 })).toMatchObject({
      primaryMode: "docked",
      auxiliaryMode: "docked",
      editorPresentation: "split",
      visibleEditorGroupCount: 2
    });
  });

  it("keeps three non-mobile panes even when both side panels are docked", () => {
    expect(resolveWorkbenchLayout({ ...base, editorGroupCount: 3 })).toMatchObject({
      primaryMode: "docked",
      auxiliaryMode: "docked",
      editorPresentation: "split",
      visibleEditorGroupCount: 3
    });
  });

  it("caps non-mobile panes at three", () => {
    expect(resolveWorkbenchLayout({ ...base, editorGroupCount: MAX_EDITOR_GROUPS + 1 })).toMatchObject({
      primaryMode: "docked",
      auxiliaryMode: "docked",
      editorPresentation: "split",
      visibleEditorGroupCount: MAX_EDITOR_GROUPS
    });
  });

  it("keeps three non-mobile panes when side panels are hidden", () => {
    expect(resolveWorkbenchLayout({ ...base, primaryOpen: false, auxiliaryOpen: false, editorGroupCount: 3 })).toMatchObject({
      primaryMode: "hidden",
      auxiliaryMode: "hidden",
      editorPresentation: "split",
      visibleEditorGroupCount: 3
    });
  });

  it("keeps mobile panels as overlays and focuses the editor", () => {
    expect(resolveWorkbenchLayout({ ...base, isMobile: true, editorGroupCount: 3 })).toMatchObject({
      primaryMode: "overlay",
      auxiliaryMode: "overlay",
      editorPresentation: "focused",
      visibleEditorGroupCount: 1
    });
  });
});
