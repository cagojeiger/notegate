import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AnchoredOverlay } from "./AnchoredOverlay";

function Harness({
  showAnchor = true,
  revision = 0
}: {
  showAnchor?: boolean;
  revision?: number;
}) {
  const anchorRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);

  return (
    <>
      {showAnchor ? (
        <button ref={anchorRef} type="button" onClick={() => setOpen(true)}>
          Open
        </button>
      ) : null}
      <AnchoredOverlay
        anchorRef={anchorRef}
        open={open}
        onClose={() => setOpen(false)}
        label="Actions"
        role="dialog"
        width={160}
        estimatedHeight={80}
      >
        <button type="button">First action</button>
        <button type="button">Second action</button>
        <span>{revision}</span>
      </AnchoredOverlay>
    </>
  );
}

describe("AnchoredOverlay", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("focuses its content and restores the anchor on Escape", async () => {
    const user = userEvent.setup();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      bottom: 48,
      height: 32,
      left: 16,
      right: 48,
      top: 16,
      width: 32,
      x: 16,
      y: 16,
      toJSON: () => ({})
    });

    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open" });
    await user.click(trigger);

    const action = screen.getByRole("button", { name: "First action" });
    expect(action).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Actions" })).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("closes when its anchor disappears", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<Harness />);

    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    rerender(<Harness showAnchor={false} />);
    expect(screen.queryByRole("dialog", { name: "Actions" })).not.toBeInTheDocument();

    rerender(<Harness />);
    expect(screen.queryByRole("dialog", { name: "Actions" })).not.toBeInTheDocument();
  });

  it("preserves focus when its parent rerenders", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<Harness />);

    await user.click(screen.getByRole("button", { name: "Open" }));
    await user.tab();
    const secondAction = screen.getByRole("button", { name: "Second action" });
    expect(secondAction).toHaveFocus();

    rerender(<Harness revision={1} />);
    expect(secondAction).toHaveFocus();
  });
});
