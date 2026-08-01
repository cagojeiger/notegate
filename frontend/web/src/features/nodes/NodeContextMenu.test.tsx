import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { copyText } from "../../shared/lib/clipboard";
import { useUiStore } from "../../stores/uiStore";
import { makeNodeSummary } from "../../test/fixtures";
import { NodeContextMenu } from "./NodeContextMenu";

vi.mock("../../shared/lib/clipboard", () => ({
  copyText: vi.fn()
}));

const lockedFolder = makeNodeSummary({
  id: "folder-1",
  name: "Policies",
  kind: "folder",
  path: "/Policies",
  has_children: true,
  effective_write_locked: true
});

describe("NodeContextMenu", () => {
  it("hides folder create actions when the space is read-only", () => {
    render(
      <NodeContextMenu
        menu={{ x: 10, y: 10, node: { ...lockedFolder, effective_write_locked: false } }}
        qualifiedPath="daily:/Policies"
        canWriteActiveSpace={false}
        onClose={vi.fn()}
        onOpenNode={vi.fn()}
        onRenameNode={vi.fn()}
        onMoveNode={vi.fn()}
        onDeleteNode={vi.fn()}
        onCreateInFolder={vi.fn()}
        onUploadInFolder={vi.fn()}
      />
    );

    const menu = within(screen.getByRole("menu"));
    expect(menu.queryByRole("button", { name: "New folder" })).not.toBeInTheDocument();
    expect(menu.queryByRole("button", { name: "New document" })).not.toBeInTheDocument();
    expect(menu.queryByText("Upload file")).not.toBeInTheDocument();
    expect(menu.getByRole("button", { name: "Copy path" })).toBeEnabled();
  });

  it("keeps read navigation available and disables every folder write action under a lock", async () => {
    const user = userEvent.setup();
    const onOpenNode = vi.fn();
    const onClose = vi.fn();
    const onCreateInFolder = vi.fn();
    const onUploadInFolder = vi.fn();
    const onRenameNode = vi.fn();
    const onMoveNode = vi.fn();
    const onDeleteNode = vi.fn();

    render(
      <NodeContextMenu
        menu={{ x: 10, y: 10, node: lockedFolder }}
        qualifiedPath="daily:/Policies"
        canWriteActiveSpace
        onClose={onClose}
        onOpenNode={onOpenNode}
        onRenameNode={onRenameNode}
        onMoveNode={onMoveNode}
        onDeleteNode={onDeleteNode}
        onCreateInFolder={onCreateInFolder}
        onUploadInFolder={onUploadInFolder}
      />
    );

    const menu = within(screen.getByRole("menu"));
    expect(menu.getByRole("button", { name: "New folder" })).toBeDisabled();
    expect(menu.getByRole("button", { name: "New document" })).toBeDisabled();
    expect(menu.getByText("Upload file").closest("label")?.querySelector("input")).toBeDisabled();
    expect(menu.getByRole("button", { name: "Rename" })).toBeDisabled();
    expect(menu.getByRole("button", { name: "Move…" })).toBeDisabled();
    expect(menu.getByRole("button", { name: "Delete" })).toBeDisabled();

    await user.click(menu.getByRole("button", { name: "Open" }));
    expect(onOpenNode).toHaveBeenCalledWith(lockedFolder);
    expect(onClose).toHaveBeenCalledOnce();
    expect(onCreateInFolder).not.toHaveBeenCalled();
    expect(onUploadInFolder).not.toHaveBeenCalled();
    expect(onRenameNode).not.toHaveBeenCalled();
    expect(onMoveNode).not.toHaveBeenCalled();
    expect(onDeleteNode).not.toHaveBeenCalled();
  });

  it("copies the space-qualified path even when the node is locked", async () => {
    const user = userEvent.setup();
    vi.mocked(copyText).mockResolvedValue(true);
    useUiStore.setState(useUiStore.getInitialState(), true);

    render(
      <NodeContextMenu
        menu={{ x: 10, y: 10, node: lockedFolder }}
        qualifiedPath="daily:/Policies"
        canWriteActiveSpace
        onClose={vi.fn()}
        onOpenNode={vi.fn()}
        onRenameNode={vi.fn()}
        onMoveNode={vi.fn()}
        onDeleteNode={vi.fn()}
        onCreateInFolder={vi.fn()}
        onUploadInFolder={vi.fn()}
      />
    );

    await user.click(screen.getByRole("button", { name: "Copy path" }));

    expect(copyText).toHaveBeenCalledWith("daily:/Policies");
    expect(useUiStore.getState().toast).toBe("Path copied");
  });
});
