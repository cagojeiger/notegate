import { StrictMode, useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { Button } from "./Button";
import { Modal } from "./Modal";

function ModalHarness({ autoFocusInput = false }: { autoFocusInput?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button onClick={() => setOpen(true)}>Open modal</Button>
      {open ? (
        <Modal
          title="Accessible modal"
          onClose={() => setOpen(false)}
          footer={<Button onClick={() => setOpen(false)}>Save</Button>}
        >
          {autoFocusInput ? <input aria-label="Name" autoFocus /> : <p>Modal content</p>}
        </Modal>
      ) : null}
    </>
  );
}

describe("Modal", () => {
  it("exposes dialog semantics, traps focus, and restores focus on close", async () => {
    const user = userEvent.setup();
    render(<ModalHarness />);

    const trigger = screen.getByRole("button", { name: "Open modal" });
    await user.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Accessible modal" });
    const close = screen.getByRole("button", { name: "Close" });
    const save = screen.getByRole("button", { name: "Save" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    await waitFor(() => expect(close).toHaveFocus());

    await user.tab({ shift: true });
    expect(save).toHaveFocus();
    await user.tab();
    expect(close).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("preserves explicit autofocus and restores the opener", async () => {
    const user = userEvent.setup();
    render(
      <StrictMode>
        <ModalHarness autoFocusInput />
      </StrictMode>
    );

    const trigger = screen.getByRole("button", { name: "Open modal" });
    await user.click(trigger);

    await waitFor(() => expect(screen.getByRole("textbox", { name: "Name" })).toHaveFocus());

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});
