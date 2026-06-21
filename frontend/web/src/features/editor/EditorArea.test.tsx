import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { EditorArea } from "./EditorArea";
import type { EditorGroup } from "../../stores/uiStore";

function renderEditorArea(overrides: Partial<Parameters<typeof EditorArea>[0]> = {}) {
  const groups: EditorGroup[] = [
    { id: 0, node: null, mode: "preview" },
    { id: 1, node: null, mode: "preview" },
    { id: 2, node: null, mode: "preview" }
  ];

  return render(
    <EditorArea
      groups={groups}
      activeGroupIndex={2}
      presentation="split"
      visibleGroupCount={groups.length}
      activeSpace={null}
      canWriteActiveSpace={false}
      onFocusGroup={vi.fn()}
      onOpenNode={vi.fn()}
      onOpenNodeInNewGroup={vi.fn()}
      onOpenMarkdownLink={vi.fn()}
      onCloseGroup={vi.fn()}
      onSetGroupMode={vi.fn()}
      onCreateFolder={vi.fn()}
      onCreateText={vi.fn()}
      onFileSelected={vi.fn()}
      onDownloadFile={vi.fn()}
      onRenameNode={vi.fn()}
      onMoveNode={vi.fn()}
      onDeleteNode={vi.fn()}
      {...overrides}
    />
  );
}

describe("EditorArea", () => {
  it("keeps the active group and its neighbor visible when visible groups are capped", () => {
    const { container } = renderEditorArea({ visibleGroupCount: 2 });

    const groups = Array.from(container.querySelectorAll("[data-editor-group]"));
    expect(groups).toHaveLength(3);
    expect(groups[0]).toHaveClass("hidden");
    expect(groups[1]).toHaveClass("flex");
    expect(groups[2]).toHaveClass("flex");
  });

  it("only shows the active group in focused presentation", () => {
    const { container } = renderEditorArea({ activeGroupIndex: 1, presentation: "focused", visibleGroupCount: 1 });

    const groups = Array.from(container.querySelectorAll("[data-editor-group]"));
    expect(groups[0]).toHaveClass("hidden");
    expect(groups[1]).toHaveClass("flex");
    expect(groups[2]).toHaveClass("hidden");
  });
});
