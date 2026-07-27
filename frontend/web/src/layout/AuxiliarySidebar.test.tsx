import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { makeRestNode } from "../test/fixtures";
import { AuxiliarySidebar } from "./AuxiliarySidebar";

const mocks = vi.hoisted(() => ({
  useFolderChildrenStat: vi.fn()
}));

vi.mock("../features/editor/useEditorQueries", () => ({
  useFolderChildrenStat: mocks.useFolderChildrenStat
}));

type SidebarProps = ComponentProps<typeof AuxiliarySidebar>;

function renderSidebar(overrides: Partial<SidebarProps> = {}) {
  const props: SidebarProps = {
    activeNode: textNode,
    canWriteActiveSpace: true,
    canManageActiveSpace: true,
    textEncryptionAvailable: true,
    writeLockAvailable: true,
    searchPolicyPending: false,
    writeLockPending: false,
    textEncryptionPending: false,
    onReplaceMetadata: vi.fn(),
    onSearchEnabledChange: vi.fn(),
    onWriteLockedChange: vi.fn(),
    onTextEncryptionEnabledChange: vi.fn(),
    ...overrides
  };
  const view = render(<AuxiliarySidebar {...props} />);
  return {
    ...view,
    rerenderSidebar(nextOverrides: Partial<SidebarProps>) {
      view.rerender(<AuxiliarySidebar {...props} {...nextOverrides} />);
    }
  };
}

describe("AuxiliarySidebar", () => {
  beforeEach(() => {
    mocks.useFolderChildrenStat.mockReturnValue({
      data: undefined,
      isError: false
    });
  });

  it("uses the shared workbench body-header height and seam", () => {
    renderSidebar({
      activeNode: null,
      canWriteActiveSpace: false,
      canManageActiveSpace: false,
      textEncryptionAvailable: false,
      writeLockAvailable: false
    });

    expect(screen.getByText("Inspector")).toHaveClass("h-12", "border-b", "border-seam");
  });

  it("changes search, write lock, and stored-text encryption independently", async () => {
    const user = userEvent.setup();
    const onSearchEnabledChange = vi.fn();
    const onWriteLockedChange = vi.fn();
    const onTextEncryptionEnabledChange = vi.fn();

    renderSidebar({
      onSearchEnabledChange,
      onWriteLockedChange,
      onTextEncryptionEnabledChange
    });

    const search = screen.getByRole("switch", { name: "Include in search" });
    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    const writeLock = screen.getByRole("switch", { name: "Lock this node" });
    expect(search).toBeChecked();
    expect(encryption).not.toBeChecked();

    await user.click(search);
    await user.click(encryption);
    await user.click(writeLock);

    expect(onSearchEnabledChange).toHaveBeenCalledWith(false);
    expect(onTextEncryptionEnabledChange).toHaveBeenCalledWith(true);
    expect(onWriteLockedChange).toHaveBeenCalledWith(true);
  });

  it("explains that node settings apply immediately", () => {
    renderSidebar();

    expect(screen.getByRole("button", { name: "About Node settings" })).toHaveAccessibleDescription(
      "Changes apply immediately to this node. A direct lock protects this node and its descendants; inherited locks must be removed at their source. Search and stored text encryption are independent settings. The space root cannot be locked."
    );
  });

  it("prioritizes node identity and keeps secondary details collapsed", () => {
    renderSidebar();

    expect(screen.getByText("Text")).toBeInTheDocument();
    expect(screen.getByText("1.8 KB")).toBeInTheDocument();
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("Details").closest("details")).not.toHaveAttribute("open");
  });

  it("shows direct child count for folders without text-only fields", () => {
    mocks.useFolderChildrenStat.mockReturnValue({
      data: {
        children: [makeRestNode({ id: "child-1" }), makeRestNode({ id: "child-2" })],
        page: { has_more: true }
      },
      isError: false
    });

    renderSidebar({
      activeNode: makeRestNode({
        id: "folder-1",
        kind: "folder",
        name: "Policies",
        path: "/Policies",
        byte_len: undefined,
        line_count: undefined
      })
    });

    expect(screen.getByText("Folder")).toBeInTheDocument();
    expect(screen.getByText("2+")).toBeInTheDocument();
    expect(screen.queryByText("Size")).not.toBeInTheDocument();
    expect(screen.queryByText("Lines")).not.toBeInTheDocument();
  });

  it("shows actual encrypted storage and allows disabling it after a tier downgrade", async () => {
    const user = userEvent.setup();
    const onTextEncryptionEnabledChange = vi.fn();

    renderSidebar({
      activeNode: {
        ...textNode,
        text_at_rest_encryption: "server"
      },
      textEncryptionAvailable: false,
      writeLockAvailable: false,
      onTextEncryptionEnabledChange
    });

    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    expect(encryption).toBeChecked();
    expect(encryption).toBeEnabled();
    await user.click(encryption);
    expect(onTextEncryptionEnabledChange).toHaveBeenCalledWith(false);
  });

  it("keeps metadata editing available while policy controls require manage access", () => {
    renderSidebar({ canManageActiveSpace: false });

    expect(screen.getByRole("button", { name: "Edit metadata" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
  });

  it("tracks search and encryption requests independently", () => {
    renderSidebar({ searchPolicyPending: true });

    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeEnabled();
  });

  it("tracks write-lock requests independently", () => {
    renderSidebar({ writeLockPending: true });

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeEnabled();
  });

  it("shows client-encrypted text as encrypted without offering a server rewrite", () => {
    renderSidebar({
      activeNode: {
        ...textNode,
        text_storage_format: "encrypted"
      }
    });

    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    expect(encryption).toBeChecked();
    expect(encryption).toBeDisabled();
    expect(screen.getByText("Client")).toBeInTheDocument();
  });

  it("shows direct protection and reveals inherited sources in an overlay", async () => {
    const user = userEvent.setup();
    renderSidebar({
      activeNode: {
        ...textNode,
        write_locked: true,
        effective_write_locked: true,
        write_lock_sources: [
          { node_id: "folder-1", name: "Policies", path: "/Policies" },
          { node_id: textNode.id, name: textNode.name, path: textNode.path }
        ]
      }
    });

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeChecked();
    expect(screen.getByText("Locked here")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit metadata" })).toBeDisabled();
    expect(screen.queryByTitle("/Policies")).not.toBeInTheDocument();

    const sourceTrigger = screen.getByRole("button", { name: "1 inherited" });
    await user.click(sourceTrigger);

    expect(screen.getByRole("dialog", { name: "Inherited lock sources" })).toBeInTheDocument();
    expect(screen.getByTitle("/Policies")).toHaveTextContent("/Policies");
    expect(screen.queryByTitle(textNode.path)).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Inherited lock sources" })).not.toBeInTheDocument();
    expect(sourceTrigger).toHaveFocus();
  });

  it("shows inherited protection without marking the node as directly locked", async () => {
    const user = userEvent.setup();
    renderSidebar({
      activeNode: {
        ...textNode,
        effective_write_locked: true,
        write_lock_sources: [
          { node_id: "folder-1", name: "Policies", path: "/Policies" }
        ]
      }
    });

    expect(screen.getByText("Inherited")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Lock this node" })).not.toBeChecked();
    expect(screen.getByRole("button", { name: "Edit metadata" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.queryByTitle("/Policies")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "1 source" }));
    expect(screen.getByTitle("/Policies")).toHaveTextContent("/Policies");
  });

  it("closes inherited lock sources when the selected node changes", async () => {
    const user = userEvent.setup();
    const { rerenderSidebar } = renderSidebar({
      activeNode: {
        ...textNode,
        effective_write_locked: true,
        write_lock_sources: [
          { node_id: "folder-1", name: "Policies", path: "/Policies" }
        ]
      }
    });

    await user.click(screen.getByRole("button", { name: "1 source" }));
    expect(screen.getByRole("dialog", { name: "Inherited lock sources" })).toBeInTheDocument();

    rerenderSidebar({
      activeNode: {
        ...textNode,
        id: "text-2",
        name: "other.md",
        path: "/other.md",
        effective_write_locked: true,
        write_lock_sources: [
          { node_id: "folder-2", name: "Archive", path: "/Archive" }
        ]
      }
    });

    expect(screen.queryByRole("dialog", { name: "Inherited lock sources" })).not.toBeInTheDocument();
  });

  it("shows an unavailable plan without naming a specific tier", () => {
    renderSidebar({
      textEncryptionAvailable: false,
      writeLockAvailable: false
    });

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
    expect(screen.getAllByText("Unavailable")).toHaveLength(2);
    expect(screen.queryByText("Max")).not.toBeInTheDocument();
  });

  it("keeps the space root lock control disabled", () => {
    renderSidebar({
      activeNode: {
        ...textNode,
        parent_id: null,
        name: "/",
        path: "/"
      }
    });

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
    expect(screen.getByText("Root")).toBeInTheDocument();
  });

  it("allows an existing direct lock to be removed after the feature becomes unavailable", async () => {
    const user = userEvent.setup();
    const onWriteLockedChange = vi.fn();
    renderSidebar({
      activeNode: {
        ...textNode,
        write_locked: true,
        effective_write_locked: true,
        write_lock_sources: [
          { node_id: textNode.id, name: textNode.name, path: textNode.path }
        ]
      },
      writeLockAvailable: false,
      onWriteLockedChange
    });

    const writeLock = screen.getByRole("switch", { name: "Lock this node" });
    expect(writeLock).toBeChecked();
    expect(writeLock).toBeEnabled();
    expect(screen.queryByText("Unavailable")).not.toBeInTheDocument();

    await user.click(writeLock);
    expect(onWriteLockedChange).toHaveBeenCalledWith(false);
  });

  it("uses an explicit empty metadata state", () => {
    renderSidebar();

    expect(screen.getByText("No metadata.")).toBeInTheDocument();
    expect(screen.queryByText("{}")).not.toBeInTheDocument();
  });
});

const textNode = makeRestNode({
  byte_len: 1842,
  line_count: 42,
  text_storage_format: "plain",
  text_at_rest_encryption: "none",
  created_at: "2026-07-26T00:00:00Z",
  updated_at: "2026-07-26T00:00:00Z"
});
