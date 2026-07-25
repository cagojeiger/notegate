import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { EditorArea } from "./EditorArea";
import type { RestNode } from "../../api/types";
import type { EditorGroup } from "../../stores/uiStore";

vi.mock("./OpenedNodeGuard", () => ({
  OpenedNodeGuard: ({ node, children }: { node: RestNode; children: (node: RestNode) => ReactNode }) => children(node)
}));

vi.mock("./FileDetailView", () => ({
  FileDetailView: () => <div>File detail</div>
}));

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

  it("clips editor groups to the workbench viewport", () => {
    const { container } = renderEditorArea();

    expect(container.firstElementChild).toHaveClass("min-h-0", "overflow-hidden");
    for (const group of container.querySelectorAll("[data-editor-group]")) {
      expect(group).toHaveClass("min-h-0", "overflow-hidden");
    }
  });

  it("downloads files from the editor header", () => {
    const node = fileNode();
    const onDownloadFile = vi.fn();
    renderEditorArea({
      groups: [{ id: 0, node, mode: "preview" }],
      activeGroupIndex: 0,
      onDownloadFile
    });

    fireEvent.click(screen.getByRole("button", { name: "Download" }));

    expect(onDownloadFile).toHaveBeenCalledWith(node);
  });
});

function fileNode(): RestNode {
  return {
    id: "file-1",
    space_id: "space-1",
    parent_id: "root-1",
    name: "document.pdf",
    kind: "file",
    path: "/document.pdf",
    sort_order: 0,
    metadata: {},
    has_children: false,
    byte_len: 29,
    media_type: "application/pdf",
    detected_media_type: "application/pdf",
    preview_available: false,
    file_preview_kind: "pdf",
    encryption_mode: "none",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  };
}
