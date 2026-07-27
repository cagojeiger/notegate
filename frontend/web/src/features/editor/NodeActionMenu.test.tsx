import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NodeActionMenu } from "./NodeActionMenu";

function renderMenu(disabled = false) {
  const props = {
    onRenameNode: vi.fn(),
    onMoveNode: vi.fn(),
    onDeleteNode: vi.fn(),
    disabled
  };
  const view = render(<NodeActionMenu {...props} />);
  return {
    ...view,
    rerenderMenu(nextDisabled: boolean) {
      view.rerender(
        <NodeActionMenu
          onRenameNode={vi.fn()}
          onMoveNode={vi.fn()}
          onDeleteNode={vi.fn()}
          disabled={nextDisabled}
        />
      );
    }
  };
}

describe("NodeActionMenu", () => {
  it("uses dialog semantics and closes when the node becomes disabled", async () => {
    const user = userEvent.setup();
    const { rerenderMenu } = renderMenu();
    const trigger = screen.getByRole("button", { name: "Node actions" });

    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("dialog", { name: "Node actions" })).toBeInTheDocument();

    rerenderMenu(true);
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Node actions" })).not.toBeInTheDocument();
    });
    expect(trigger).toBeDisabled();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("does not reset action focus when its parent rerenders", async () => {
    const user = userEvent.setup();
    const { rerenderMenu } = renderMenu();

    await user.click(screen.getByRole("button", { name: "Node actions" }));
    expect(screen.getByRole("button", { name: "Rename" })).toHaveFocus();

    await user.tab();
    const move = screen.getByRole("button", { name: "Move" });
    expect(move).toHaveFocus();

    rerenderMenu(false);
    expect(move).toHaveFocus();
  });
});
