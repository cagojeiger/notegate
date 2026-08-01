import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RestNode } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import type { EditorGroup } from "../../stores/uiStore";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode, makeSpace } from "../../test/fixtures";
import { EditorArea } from "./EditorArea";

vi.mock("./OpenedNodeGuard", () => ({
  OpenedNodeGuard: ({ node, children }: { node: RestNode; children: (node: RestNode) => ReactNode }) => children(node)
}));

vi.mock("./FileDetailView", () => ({
  FileDetailView: () => <div>File detail</div>
}));

vi.mock("../../shared/lib/clipboard", () => ({
  copyText: vi.fn()
}));

function renderEditorArea(overrides: Partial<Parameters<typeof EditorArea>[0]> = {}) {
  const groups: EditorGroup[] = [
    { id: 0, node: null, mode: "preview", back: [], forward: [] },
    { id: 1, node: null, mode: "preview", back: [], forward: [] },
    { id: 2, node: null, mode: "preview", back: [], forward: [] }
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
      onNavigateEditorGroup={vi.fn()}
      navigatingGroupIds={new Set()}
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
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(copyText).mockReset();
    vi.mocked(copyText).mockResolvedValue(true);
  });

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
      groups: [{ id: 0, node, mode: "preview", back: [], forward: [] }],
      activeGroupIndex: 0,
      onDownloadFile
    });

    fireEvent.click(screen.getByRole("button", { name: "Download" }));

    expect(onDownloadFile).toHaveBeenCalledWith(node);
  });

  it("keeps locked files downloadable and their qualified path copyable while disabling node mutations", async () => {
    const node = { ...fileNode(), effective_write_locked: true };
    const onDownloadFile = vi.fn();
    renderEditorArea({
      groups: [{ id: 0, node, mode: "preview", back: [], forward: [] }],
      activeGroupIndex: 0,
      activeSpace: makeSpace({ name: "daily" }),
      canWriteActiveSpace: true,
      onDownloadFile
    });

    const download = screen.getByRole("button", { name: "Download" });
    const copyPath = screen.getByRole("button", { name: "Copy path" });
    expect(download).toBeEnabled();
    expect(copyPath).toBeEnabled();
    expect(screen.getByRole("button", { name: "More actions" })).toBeDisabled();

    fireEvent.click(download);
    fireEvent.click(copyPath);
    expect(onDownloadFile).toHaveBeenCalledWith(node);
    await waitFor(() => {
      expect(copyText).toHaveBeenCalledWith("daily:/document.pdf");
      expect(useUiStore.getState().toast).toBe("Path copied");
    });
  });

  it("shows per-group navigation controls before the title", () => {
    const current = fileNode();
    const onNavigateEditorGroup = vi.fn();
    renderEditorArea({
      groups: [{
        id: 7,
        node: current,
        mode: "preview",
        back: [{ spaceId: current.space_id, nodeId: "previous", nameSnapshot: "previous.md", kind: "text" }],
        forward: []
      }],
      activeGroupIndex: 0,
      onNavigateEditorGroup
    });

    const title = screen.getByText(current.name);
    const back = screen.getByRole("button", { name: "Back to previous.md" });
    const forward = screen.getByRole("button", { name: "Forward" });
    expect(back.compareDocumentPosition(title) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(back).toBeEnabled();
    expect(forward).toBeDisabled();

    fireEvent.click(back);
    expect(onNavigateEditorGroup).toHaveBeenCalledWith(7, "back");
  });
});

function fileNode(): RestNode {
  return makeRestNode({
    id: "file-1",
    name: "document.pdf",
    kind: "file",
    path: "/document.pdf",
    byte_len: 29,
    media_type: "application/pdf",
    detected_media_type: "application/pdf",
    preview_available: false,
    file_preview_kind: "pdf",
    encryption_mode: "none"
  });
}
