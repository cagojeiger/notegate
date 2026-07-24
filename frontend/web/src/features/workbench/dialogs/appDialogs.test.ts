import { describe, expect, it, vi } from "vitest";

import type { RestNode } from "../../../entities/node/model";
import type { Space } from "../../../entities/space/model";
import { createNodeDialog, deleteNodeDialog, renameNodeDialog, renameSpaceDialog, uploadFileDialog } from "./appDialogs";

const space: Space = {
  id: "space-1",
  name: "Personal",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

function node(overrides: Partial<RestNode> = {}): RestNode {
  return {
    id: "node-1",
    space_id: space.id,
    parent_id: space.root_node_id,
    name: "note.md",
    kind: "text",
    path: "/note.md",
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z",
    ...overrides
  };
}

describe("app dialog builders", () => {
  it("ignores unchanged rename submissions", () => {
    const onRenameSpace = vi.fn();
    const spaceDialog = renameSpaceDialog(space, onRenameSpace);
    if (spaceDialog.kind !== "prompt") throw new Error("expected prompt dialog");
    spaceDialog.onSubmit("Personal");
    spaceDialog.onSubmit("Work");

    expect(onRenameSpace).toHaveBeenCalledTimes(1);
    expect(onRenameSpace).toHaveBeenCalledWith(space.id, "Work");

    const onRenameNode = vi.fn();
    const textNode = node();
    const nodeDialog = renameNodeDialog(textNode, onRenameNode);
    if (nodeDialog.kind !== "prompt") throw new Error("expected prompt dialog");
    nodeDialog.onSubmit("note.md");
    nodeDialog.onSubmit("renamed.md");

    expect(onRenameNode).toHaveBeenCalledTimes(1);
    expect(onRenameNode).toHaveBeenCalledWith(textNode, "renamed.md");
  });

  it("creates folders without content and texts with empty content", () => {
    const onCreate = vi.fn();
    const folderDialog = createNodeDialog("parent-1", "folder", onCreate);
    const textDialog = createNodeDialog("parent-1", "text", onCreate);

    if (folderDialog.kind !== "prompt" || textDialog.kind !== "prompt") throw new Error("expected prompt dialogs");
    folderDialog.onSubmit("docs");
    textDialog.onSubmit("daily.md");

    expect(onCreate).toHaveBeenNthCalledWith(1, { parentId: "parent-1", kind: "folder", name: "docs", content: undefined });
    expect(onCreate).toHaveBeenNthCalledWith(2, { parentId: "parent-1", kind: "text", name: "daily.md", content: "" });
  });

  it("keeps file and parent context in upload dialogs", () => {
    const onUpload = vi.fn();
    const file = new File(["hello"], "hello.txt", { type: "text/plain" });
    const dialog = uploadFileDialog("parent-1", file, onUpload);

    if (dialog.kind !== "prompt") throw new Error("expected prompt dialog");
    expect(dialog.initial).toBe("hello.txt");
    dialog.onSubmit("renamed.txt");

    expect(onUpload).toHaveBeenCalledWith({ parentId: "parent-1", name: "renamed.txt", file });
  });

  it("marks folder delete as recursive in the confirmation callback", () => {
    const onDelete = vi.fn();
    const folder = node({ kind: "folder", name: "docs" });
    const dialog = deleteNodeDialog(folder, onDelete);

    if (dialog.kind !== "confirm") throw new Error("expected confirm dialog");
    expect(dialog.message).toContain("everything inside it");
    dialog.onConfirm();

    expect(onDelete).toHaveBeenCalledWith(folder, true);
  });
});
