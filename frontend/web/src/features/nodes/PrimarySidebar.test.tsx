import type { ComponentProps } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PrimarySidebar } from "./PrimarySidebar";

const props: ComponentProps<typeof PrimarySidebar> = {
  activeSpace: null,
  openedNodeId: null,
  inspectedNodeId: null,
  expandedFolderIds: new Set(),
  canWriteActiveSpace: false,
  canManageActiveSpace: false,
  canOpenInNewGroup: false,
  onToggleFolder: vi.fn(),
  onInspectNode: vi.fn(),
  onOpenNode: vi.fn(),
  onOpenNodeInNewGroup: vi.fn(),
  onCreateFolder: vi.fn(),
  onCreateText: vi.fn(),
  onRecordAudio: vi.fn(),
  onFileSelected: vi.fn(),
  onRenameSpace: vi.fn(),
  onDeleteSpace: vi.fn(),
  onRenameNode: vi.fn(),
  onMoveNode: vi.fn(),
  onMoveNodeToFolder: vi.fn(),
  onDeleteNode: vi.fn(),
  onDownloadFile: vi.fn(),
  onCollapseTree: vi.fn(),
  onCreateInFolder: vi.fn(),
  onUploadInFolder: vi.fn()
};

describe("PrimarySidebar", () => {
  it("exposes Browse as the selected sidebar panel", () => {
    render(<PrimarySidebar {...props} />);

    const tab = screen.getByRole("tab", { name: "Browse" });
    const panel = screen.getByRole("tabpanel", { name: "Browse" });

    expect(tab).toHaveAttribute("aria-selected", "true");
    expect(tab).toHaveAttribute("aria-controls", "browse-sidebar-panel");
    expect(panel).toHaveAttribute("id", "browse-sidebar-panel");
    expect(panel).toHaveTextContent("Create a space to start.");
  });
});
