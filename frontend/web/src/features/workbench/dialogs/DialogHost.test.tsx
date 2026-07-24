import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { RestNode } from "../../../api/types";
import { DialogHost } from "./DialogHost";

const textNode: RestNode = {
  id: "node-1",
  space_id: "space-1",
  parent_id: "root",
  name: "note.md",
  kind: "text",
  path: "/note.md",
  sort_order: 0,
  metadata: { title: "note" },
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

describe("DialogHost", () => {
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

  it("validates metadata JSON before saving", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    const onClose = vi.fn();

    render(<DialogHost dialog={{ kind: "metadata", node: textNode, onSave }} onClose={onClose} />);

    const textarea = screen.getByRole("textbox");
    const save = screen.getByRole("button", { name: "Save" });

    await user.clear(textarea);
    await user.type(textarea, "not json");
    expect(save).toBeDisabled();
    expect(screen.getAllByText(/not valid JSON/i).length).toBeGreaterThan(0);

    await user.clear(textarea);
    await user.click(textarea);
    await user.paste(JSON.stringify({ title: "updated" }));
    await user.click(save);

    expect(onSave).toHaveBeenCalledWith({ title: "updated" });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
