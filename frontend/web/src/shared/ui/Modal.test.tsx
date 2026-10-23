import { StrictMode, useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { Button } from "./Button";
import { HelpTooltip } from "./HelpTooltip";
import { Modal } from "./Modal";

function ModalHarness({
  autoFocusInput = false,
  withTooltip = false
}: {
  autoFocusInput?: boolean;
  withTooltip?: boolean;
}) {
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
          {autoFocusInput ? <input aria-label="Name" autoFocus /> : null}
          {withTooltip ? (
            <HelpTooltip label="About modal help">Modal help</HelpTooltip>
          ) : null}
          {!autoFocusInput && !withTooltip ? <p>Modal content</p> : null}
        </Modal>
      ) : null}
    </>
  );
}

describe("Modal", () => {
  it("includes native disclosure summaries in both directions of the focus loop", async () => {
    const user = userEvent.setup();
    render(<Modal title="Call details" onClose={() => undefined}>
      <details><summary>Input</summary><p>Request data</p></details>
      <details><summary>Response</summary><p>Response data</p></details>
    </Modal>);
    const close = screen.getByRole("button", { name: "Close" });
    const input = screen.getByText("Input");
    const response = screen.getByText("Response");
    expect(close).toHaveFocus();
    await user.tab();
    expect(input).toHaveFocus();
    await user.tab();
    expect(response).toHaveFocus();
    await user.tab();
    expect(close).toHaveFocus();
    await user.tab({ shift: true });
    expect(response).toHaveFocus();
  });

  it("keeps the focus loop on tabbable controls when tabs or fieldsets exclude buttons", async () => {
    const user = userEvent.setup();
    render(<>
      <Modal title="Navigation" onClose={() => undefined}>
        <button>Active tab</button>
        <button tabIndex={-1}>Inactive tab</button>
        <fieldset disabled><button>Disabled action</button></fieldset>
      </Modal>
      <button>Outside dialog</button>
    </>);
    const active = screen.getByRole("button", { name: "Active tab" });
    const close = screen.getByRole("button", { name: "Close" });
    active.focus();
    await user.tab();
    expect(close).toHaveFocus();
    await user.tab({ shift: true });
    expect(active).toHaveFocus();
  });

  it("exposes dialog semantics, traps focus, and restores focus on close", async () => {
    const user = userEvent.setup();
    render(<ModalHarness />);

    const trigger = screen.getByRole("button", { name: "Open modal" });
    await user.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Accessible modal" });
    const close = screen.getByRole("button", { name: "Close" });
    const save = screen.getByRole("button", { name: "Save" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveClass("font-ui", "text-workbench");
    expect(dialog).toHaveClass("rounded-workbench-surface");
    expect(screen.getByRole("heading", { name: "Accessible modal" })).toHaveClass("text-base");
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

  it("dismisses nested help before closing the dialog", async () => {
    const user = userEvent.setup();
    render(<ModalHarness withTooltip />);

    await user.click(screen.getByRole("button", { name: "Open modal" }));
    await user.click(screen.getByRole("button", { name: "About modal help" }));
    expect(screen.getByRole("tooltip")).toBeVisible();

    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "Accessible modal" })).toBeInTheDocument();
    expect(screen.getByRole("tooltip", { hidden: true })).not.toBeVisible();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("preserves focus moved to a new view by a dialog action", async () => {
    function Harness() {
      const [view, setView] = useState("closed");
      return <>
        <Button onClick={() => setView("dialog")}>Open modal</Button>
        {view === "dialog" ? <Modal title="Choose action" onClose={() => setView("closed")}>
          <Button onClick={() => setView("editor")}>Create item</Button>
        </Modal> : null}
        {view === "editor" ? <input autoFocus aria-label="New item name" /> : null}
      </>;
    }
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Open modal" }));
    await user.click(screen.getByRole("button", { name: "Create item" }));
    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "New item name" })).toHaveFocus();
  });

  it("keeps bottom-sheet content above the device safe area", () => {
    render(
      <Modal title="Bottom sheet" placement="bottom" onClose={() => undefined}>
        <p>Sheet content</p>
      </Modal>
    );

    const dialog = screen.getByRole("dialog", { name: "Bottom sheet" });
    const content = screen.getByText("Sheet content").parentElement;
    expect(dialog).toHaveClass("h-[82dvh]");
    expect(content).toHaveClass("pb-[env(safe-area-inset-bottom)]");
  });
});
