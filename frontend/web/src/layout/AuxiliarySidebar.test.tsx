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
        searchPolicyPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByText("Inspector")).toHaveClass("h-12", "border-b", "border-seam");
  });

  it("changes search and stored-text encryption independently", async () => {
    const user = userEvent.setup();
    const onSearchEnabledChange = vi.fn();
    const onTextEncryptionEnabledChange = vi.fn();

    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        searchPolicyPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={onSearchEnabledChange}
        onTextEncryptionEnabledChange={onTextEncryptionEnabledChange}
      />
    );

    const search = screen.getByRole("switch", { name: "Include in search" });
    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    expect(search).toBeChecked();
    expect(encryption).not.toBeChecked();

    await user.click(search);
    await user.click(encryption);

    expect(onSearchEnabledChange).toHaveBeenCalledWith(false);
    expect(onTextEncryptionEnabledChange).toHaveBeenCalledWith(true);
  });

  it("explains that node settings apply immediately", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        searchPolicyPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "About Node settings" })).toHaveAccessibleDescription(
      "Changes apply immediately to this node. Search controls whether it appears in find and grep results. Stored text encryption applies only to text content. The settings are independent."
    );
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
        searchPolicyPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
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
        searchPolicyPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Edit metadata" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Stored text encryption" })).toBeDisabled();
  });

  it("tracks search and encryption requests independently", () => {
    render(
      <AuxiliarySidebar
        activeNode={textNode}
        canWriteActiveSpace
        canManageActiveSpace
        textEncryptionAvailable
        searchPolicyPending
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    expect(screen.getByRole("switch", { name: "Include in search" })).toBeDisabled();
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
        searchPolicyPending={false}
        textEncryptionPending={false}
        onReplaceMetadata={vi.fn()}
        onSearchEnabledChange={vi.fn()}
        onTextEncryptionEnabledChange={vi.fn()}
      />
    );

    const encryption = screen.getByRole("switch", { name: "Stored text encryption" });
    expect(encryption).toBeChecked();
    expect(encryption).toBeDisabled();
    expect(screen.getByText("Client")).toBeInTheDocument();
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
  has_children: false,
  text_storage_format: "plain",
  text_at_rest_encryption: "none",
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-07-26T00:00:00Z",
  updated_at: "2026-07-26T00:00:00Z"
};
