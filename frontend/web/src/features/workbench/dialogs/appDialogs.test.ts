import { describe, expect, it, vi } from "vitest";

import type { RestNode } from "../../../api/types";
import { makeRestNode, makeSpace } from "../../../test/fixtures";
import { createNodeDialog, deleteNodeDialog, renameNodeDialog, renameSpaceDialog, uploadFileDialog } from "./appDialogs";

const space = makeSpace({
  name: "Personal",
});

function node(overrides: Partial<RestNode> = {}): RestNode {
  return makeRestNode({
    space_id: space.id,
    parent_id: space.root_node_id,
    ...overrides
  });
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
