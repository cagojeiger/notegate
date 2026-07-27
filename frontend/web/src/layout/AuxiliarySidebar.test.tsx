import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { RestNode } from "../api/types";
import { AuxiliarySidebar } from "./AuxiliarySidebar";

describe("AuxiliarySidebar", () => {
  it("uses the shared workbench body-header height and seam", () => {
    render(
      <AuxiliarySidebar
        activeNode={null}
        canWriteActiveSpace={false}
        canManageActiveSpace={false}
        textEncryptionAvailable={false}
        writeLockAvailable={false}
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByText("Inspector")).toHaveClass("h-12", "border-b", "border-seam");
  });

  it("changes search, write lock, and stored-text encryption independently", async () => {
    const user = userEvent.setup();
    const onSearchEnabledChange = vi.fn();
    const onWriteLockedChange = vi.fn();
    const onTextEncryptionEnabledChange = vi.fn();

    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={onSearchEnabledChange}
        onWriteLockedChange={onWriteLockedChange}
        onTextEncryptionEnabledChange={onTextEncryptionEnabledChange}
      />
    );

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
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "About Node settings" })).toHaveAccessibleDescription(
      "Changes apply immediately to this node. A direct lock protects this node and its descendants; inherited locks must be removed at their source. Search and stored text encryption are independent settings. The space root cannot be locked."
    );
  });

  it("prioritizes node identity and keeps secondary details collapsed", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByText("Text · 1.8 KB · 42 lines")).toBeInTheDocument();
    expect(screen.getByText("Details").closest("details")).not.toHaveAttribute("open");
  });

  it("shows actual encrypted storage and allows disabling it after a tier downgrade", async () => {
    const user = userEvent.setup();
    const onTextEncryptionEnabledChange = vi.fn();

    render(
      <AuxiliarySidebar
        activeNode={{
          ...textNode,
          text_at_rest_encryption: "server"
        }}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable={false}
        writeLockAvailable={false}
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={onTextEncryptionEnabledChange}
      />
    );

    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    expect(encryption).toBeChecked();
    expect(encryption).toBeEnabled();
    await user.click(encryption);
    expect(onTextEncryptionEnabledChange).toHaveBeenCalledWith(false);
  });

  it("keeps metadata editing available while policy controls require manage access", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace={false}
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Edit metadata" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
  });

  it("tracks search and encryption requests independently", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeEnabled();
  });

  it("tracks write-lock requests independently", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeEnabled();
  });

  it("shows client-encrypted text as encrypted without offering a server rewrite", () => {
    render(
      <AuxiliarySidebar
        activeNode={{
          ...textNode,
          text_storage_format: "encrypted"
        }}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    expect(encryption).toBeChecked();
    expect(encryption).toBeDisabled();
    expect(screen.getByText("Client")).toBeInTheDocument();
  });

  it("shows direct protection and reveals inherited sources in an overlay", async () => {
    const user = userEvent.setup();
    render(
      <AuxiliarySidebar
        activeNode={{
          ...textNode,
          write_locked: true,
          effective_write_locked: true,
          write_lock_sources: [
            { node_id: "folder-1", name: "Policies", path: "/Policies" },
            { node_id: textNode.id, name: textNode.name, path: textNode.path }
          ]
        }}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

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
    render(
      <AuxiliarySidebar
        activeNode={{
          ...textNode,
          effective_write_locked: true,
          write_lock_sources: [
            { node_id: "folder-1", name: "Policies", path: "/Policies" }
          ]
        }}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByText("Inherited")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Lock this node" })).not.toBeChecked();
    expect(screen.getByRole("button", { name: "Edit metadata" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.queryByTitle("/Policies")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "1 source" }));
    expect(screen.getByTitle("/Policies")).toHaveTextContent("/Policies");
  });

  it("shows an unavailable plan without naming a specific tier", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable={false}
        writeLockAvailable={false}
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(screen.queryByText("Max")).not.toBeInTheDocument();
  });

  it("keeps the space root lock control disabled", () => {
    render(
      <AuxiliarySidebar
        activeNode={{
          ...textNode,
          parent_id: null,
          name: "/",
          path: "/"
        }}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("switch", { name: "Lock this node" })).toBeDisabled();
    expect(screen.getByText("Root")).toBeInTheDocument();
  });

  it("allows an existing direct lock to be removed after the feature becomes unavailable", async () => {
    const user = userEvent.setup();
    const onWriteLockedChange = vi.fn();
    render(
      <AuxiliarySidebar
        activeNode={{
          ...textNode,
          write_locked: true,
          effective_write_locked: true,
          write_lock_sources: [
            { node_id: textNode.id, name: textNode.name, path: textNode.path }
          ]
        }}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable={false}
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={onWriteLockedChange}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    const writeLock = screen.getByRole("switch", { name: "Lock this node" });
    expect(writeLock).toBeChecked();
    expect(writeLock).toBeEnabled();
    expect(screen.queryByText("Unavailable")).not.toBeInTheDocument();

    await user.click(writeLock);
    expect(onWriteLockedChange).toHaveBeenCalledWith(false);
  });

  it("uses an explicit empty metadata state", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        writeLockAvailable
        searchPolicyPending={false}
        writeLockPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onWriteLockedChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByText("No metadata.")).toBeInTheDocument();
    expect(screen.queryByText("{}")).not.toBeInTheDocument();
  });
});

const textNode: RestNode = {
  id: "node-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "note.md",
  kind: "text",
  path: "/note.md",
  sort_order: 0,
  metadata: {},
  search_enabled: true,
  write_locked: false,
  write_lock_sources: [],
  has_children: false,
  effective_write_locked: false,
  byte_len: 1842,
  line_count: 42,
  text_storage_format: "plain",
  text_at_rest_encryption: "none",
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-07-26T00:00:00Z",
  updated_at: "2026-07-26T00:00:00Z"
};
