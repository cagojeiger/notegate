import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ChildrenResponse, RestNode } from "../../../api/types";
import { makeRestNode, makeSpace } from "../../../test/fixtures";
import { createTestQueryClient } from "../../../test/queryClient";
import { DialogHost } from "./DialogHost";
import type { AppDialog } from "./dialogTypes";

const mocks = vi.hoisted(() => ({
  apiGet: vi.fn()
}));

vi.mock("../../../api/ApiProvider", () => ({
  useApiClient: () => ({ get: mocks.apiGet })
}));

const textNode = makeRestNode({
  parent_id: "root",
  metadata: { title: "note" },
});

const space = makeSpace({
  name: "Space",
  root_node_id: "root",
});

describe("DialogHost", () => {
  beforeEach(() => {
    mocks.apiGet.mockReset();
  });

  it("submits non-empty prompt input", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const onClose = vi.fn();

    render(<DialogHost dialog={{ kind: "prompt", title: "New text", label: "Name", initial: "", submitLabel: "Create", onSubmit }} onClose={onClose} />);

    const create = screen.getByRole("button", { name: "Create" });
    expect(create).toBeDisabled();

    await user.type(screen.getByLabelText("Name"), "daily.md");
    await user.click(create);

    expect(onSubmit).toHaveBeenCalledWith("daily.md");
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("keeps prompt dialogs open when submit fails", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockRejectedValue(new Error("name already exists"));
    const onClose = vi.fn();

    render(<DialogHost dialog={{ kind: "prompt", title: "New text", label: "Name", initial: "", submitLabel: "Create", onSubmit }} onClose={onClose} />);

    await user.type(screen.getByLabelText("Name"), "daily.md");
    await user.click(screen.getByRole("button", { name: "Create" }));

    expect(await screen.findByText("name already exists")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("calls confirm action then closes", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const onClose = vi.fn();

    render(<DialogHost dialog={{ kind: "confirm", title: "Delete", message: "Delete this node?", danger: true, confirmLabel: "Delete", onConfirm }} onClose={onClose} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));

    expect(onConfirm).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("keeps confirm dialogs open when confirm fails", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn().mockRejectedValue(new Error("delete failed"));
    const onClose = vi.fn();

    render(<DialogHost dialog={{ kind: "confirm", title: "Delete", message: "Delete this node?", danger: true, confirmLabel: "Delete", onConfirm }} onClose={onClose} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));

    expect(await screen.findByText("delete failed")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("loads move destinations with the next cursor and retries a failed page", async () => {
    const user = userEvent.setup();
    mocks.apiGet
      .mockResolvedValueOnce(childrenPage([folder("folder-1", "First folder")], "next-page"))
      .mockRejectedValueOnce(new Error("page failed"));

    renderMoveDialog();

    expect(await screen.findByRole("button", { name: "First folder" })).toBeVisible();
    expect(mocks.apiGet).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/nodes/root/children?limit=100&view=summary"
    );
    expect(screen.queryByRole("button", { name: "Second folder" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Load more" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load more folders.");
    expect(mocks.apiGet).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/nodes/root/children?limit=100&view=summary&cursor=next-page"
    );

    mocks.apiGet.mockResolvedValueOnce(childrenPage([folder("folder-2", "Second folder")], null));
    await user.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("button", { name: "Second folder" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  });

  it("retries the initial move destination request after an error", async () => {
    const user = userEvent.setup();
    mocks.apiGet.mockRejectedValueOnce(new Error("initial request failed"));

    renderMoveDialog();

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load folders.");
    mocks.apiGet.mockResolvedValueOnce(childrenPage([folder("folder-1", "First folder")], null));
    await user.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("button", { name: "First folder" })).toBeVisible();
  });

  it("shows only valid move destinations and disables the current parent", async () => {
    mocks.apiGet.mockResolvedValueOnce(childrenPage([
      folder(textNode.id, "Source folder"),
      { ...textNode, id: "note-2", name: "other.md", path: "/other.md" },
      folder("archive", "Archive")
    ], null));

    renderMoveDialog();

    expect(await screen.findByRole("button", { name: "Archive" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Source folder" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "other.md" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Move here" })).toBeDisabled();
    expect(screen.getByText(/already here/i)).toBeVisible();
  });

  it("moves to a selected folder and closes the dialog", async () => {
    const user = userEvent.setup();
    const onMove = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    mocks.apiGet
      .mockResolvedValueOnce(childrenPage([folder("archive", "Archive")], null))
      .mockResolvedValueOnce(childrenPage([], null));

    renderMoveDialog({ onMove, onClose });

    await user.click(await screen.findByRole("button", { name: "Archive" }));
    await screen.findByText("No subfolders here");
    await user.click(screen.getByRole("button", { name: "Move here" }));

    expect(onMove).toHaveBeenCalledOnce();
    expect(onMove).toHaveBeenCalledWith("archive");
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("does not enter an effectively write-locked move destination", async () => {
    const user = userEvent.setup();
    mocks.apiGet.mockResolvedValueOnce(childrenPage([
      { ...folder("folder-1", "Locked folder"), effective_write_locked: true }
    ], null));

    renderMoveDialog();

    const lockedFolder = await screen.findByRole("button", { name: "Locked folder" });
    expect(lockedFolder).toBeDisabled();
    expect(lockedFolder).toHaveAttribute("title", "Write locked");
    await user.click(lockedFolder);

    expect(mocks.apiGet).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Root")).toHaveClass("font-semibold");
  });

  it("does not submit a move when the source node is write-locked", async () => {
    const user = userEvent.setup();
    const onMove = vi.fn();
    mocks.apiGet.mockResolvedValueOnce(childrenPage([], null));

    renderMoveDialog({
      node: { ...textNode, parent_id: "current-folder", effective_write_locked: true },
      onMove
    });

    const moveHere = await screen.findByRole("button", { name: "Move here" });
    expect(moveHere).toBeDisabled();
    await user.click(moveHere);

    expect(onMove).not.toHaveBeenCalled();
  });

  it("keeps the move dialog open when the backend rejects a protected subtree", async () => {
    const user = userEvent.setup();
    const onMove = vi.fn().mockRejectedValue(
      new Error("subtree contains a directly write-locked node")
    );
    const onClose = vi.fn();
    mocks.apiGet.mockResolvedValueOnce(childrenPage([], null));

    renderMoveDialog({
      node: { ...textNode, parent_id: "current-folder" },
      onMove,
      onClose
    });

    await user.click(await screen.findByRole("button", { name: "Move here" }));

    expect(await screen.findByText("subtree contains a directly write-locked node")).toBeVisible();
    expect(onMove).toHaveBeenCalledWith("root");
    expect(onClose).not.toHaveBeenCalled();
  });

});

function folder(id: string, name: string): RestNode {
  return {
    ...textNode,
    id,
    parent_id: "root",
    name,
    kind: "folder",
    path: `/${name}`,
    has_children: true,
    effective_write_locked: false
  };
}

function childrenPage(children: RestNode[], nextCursor: string | null): ChildrenResponse {
  return {
    parent: { id: "root", path: "/" },
    children,
    page: {
      limit: 100,
      returned: children.length,
      has_more: nextCursor !== null,
      next_cursor: nextCursor
    }
  };
}

function renderMoveDialog({
  node = textNode,
  onMove = vi.fn(),
  onClose = vi.fn()
}: {
  node?: RestNode;
  onMove?: Extract<AppDialog, { kind: "move" }>["onMove"];
  onClose?: () => void;
} = {}) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <DialogHost
        dialog={{
          kind: "move",
          node,
          space,
          onMove
        }}
        onClose={onClose}
      />
    </QueryClientProvider>
  );
}
