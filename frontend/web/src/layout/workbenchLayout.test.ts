import { describe, expect, it } from "vitest";

import { MAX_EDITOR_GROUPS, WORKBENCH_LAYOUT } from "../shared/model/workbenchLayout";
import { resolveSidebarWidths, resolveWorkbenchLayout } from "./workbenchLayout";

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

  it("reserves editor space while fitting wide panels into a tablet viewport", () => {
    const layout = resolveWorkbenchLayout({ ...base, viewportWidth: 900 });
    const widths = resolveSidebarWidths(520, 520, layout.sidebars);

    expect(layout.primaryMode).toBe("docked");
    expect(layout.auxiliaryMode).toBe("docked");
    expect(widths).toEqual({ primaryWidth: 256, auxiliaryWidth: 280 });
    expect(900 - WORKBENCH_LAYOUT.activityRailWidth - widths.primaryWidth - widths.auxiliaryWidth).toBe(WORKBENCH_LAYOUT.minEditorWidth);
  });

  it("overlays the inspector when the minimum panels and editor no longer fit", () => {
    const layout = resolveWorkbenchLayout({ ...base, viewportWidth: 800 });

    expect(layout.primaryMode).toBe("docked");
    expect(layout.auxiliaryMode).toBe("overlay");
    expect(layout.editorPresentation).toBe("split");
    expect(resolveSidebarWidths(520, 520, layout.sidebars)).toEqual({ primaryWidth: 436, auxiliaryWidth: 520 });
  });

  it("restores preferred widths when the viewport grows without changing preferences", () => {
    const tablet = resolveWorkbenchLayout({ ...base, viewportWidth: 900 });
    const desktop = resolveWorkbenchLayout({ ...base, viewportWidth: 1440 });

    expect(resolveSidebarWidths(520, 520, tablet.sidebars).primaryWidth).toBe(256);
    expect(resolveSidebarWidths(520, 520, desktop.sidebars)).toEqual({ primaryWidth: 520, auxiliaryWidth: 520 });
  });

  it("gives a single docked panel the remaining sidebar budget", () => {
    const primary = resolveWorkbenchLayout({ ...base, auxiliaryOpen: false, viewportWidth: 800 });
    const auxiliary = resolveWorkbenchLayout({ ...base, primaryOpen: false, viewportWidth: 800 });

    expect(resolveSidebarWidths(520, 520, primary.sidebars).primaryWidth).toBe(436);
    expect(auxiliary.auxiliaryMode).toBe("docked");
    expect(resolveSidebarWidths(520, 520, auxiliary.sidebars).auxiliaryWidth).toBe(436);
  });
});
