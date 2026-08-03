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
    const trigger = screen.getByRole("button", { name: "More actions" });

    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("dialog", { name: "More actions" })).toBeInTheDocument();

    rerenderMenu(true);
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "More actions" })).not.toBeInTheDocument();
    });
    expect(trigger).toBeDisabled();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("does not reset action focus when its parent rerenders", async () => {
    const user = userEvent.setup();
    const { rerenderMenu } = renderMenu();

    await user.click(screen.getByRole("button", { name: "More actions" }));
    expect(screen.getByRole("button", { name: "Rename" })).toHaveFocus();

    await user.tab();
    const move = screen.getByRole("button", { name: "Move" });
    expect(move).toHaveFocus();

    rerenderMenu(false);
    expect(move).toHaveFocus();
  });

  it("keeps supplemental read actions available when mutations are disabled", async () => {
    const user = userEvent.setup();
    const copyContent = vi.fn();

    render(
      <NodeActionMenu
        onRenameNode={vi.fn()}
        onMoveNode={vi.fn()}
        onDeleteNode={vi.fn()}
        disabled
        supplementalActions={[{ label: "Copy content", onClick: copyContent }]}
      />
    );

    const trigger = screen.getByRole("button", { name: "More actions" });
    expect(trigger).toBeEnabled();
    await user.click(trigger);
    await user.click(screen.getByRole("button", { name: "Copy content" }));

    expect(copyContent).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog", { name: "More actions" })).not.toBeInTheDocument();
  });
});
