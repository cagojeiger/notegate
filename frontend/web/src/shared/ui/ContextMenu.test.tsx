import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ContextMenu } from "./ContextMenu";
import { MenuButton } from "./MenuButton";

function Harness({ onSelect = () => undefined }: { onSelect?: () => void }) {
  const [open, setOpen] = useState(false);
  return <>
    <button onClick={() => setOpen(true)}>Actions</button>
    <button>Next control</button>
    {open ? <ContextMenu x={20} y={20} label="File actions" onClose={() => setOpen(false)}>
      <MenuButton disabled onClick={() => undefined}>Disabled action</MenuButton>
      <MenuButton onClick={() => { onSelect(); setOpen(false); }}>Open</MenuButton>
      <MenuButton onClick={() => setOpen(false)}>Copy path</MenuButton>
    </ContextMenu> : null}
  </>;
}

describe("ContextMenu", () => {
  it("focuses enabled items, wraps arrow navigation and restores the trigger on Escape", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Actions" });
    await user.click(trigger);
    const open = screen.getByRole("menuitem", { name: "Open" });
    const copy = screen.getByRole("menuitem", { name: "Copy path" });
    expect(open).toHaveFocus();
    await user.keyboard("{ArrowUp}");
    expect(copy).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(open).toHaveFocus();
    await user.keyboard("{End}");
    expect(copy).toHaveFocus();
    await user.keyboard("{Home}");
    expect(open).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("closes on Tab and continues to the next control outside the menu", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Actions" }));
    await user.tab();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next control" })).toHaveFocus();
  });

  it("activates an item with Enter and restores focus after closing", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);
    const trigger = screen.getByRole("button", { name: "Actions" });
    await user.click(trigger);
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("keeps plain buttons outside a context menu", () => {
    render(<MenuButton onClick={() => undefined}>Rename</MenuButton>);
    expect(screen.getByRole("button", { name: "Rename" })).not.toHaveAttribute("tabindex");
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
  });
});
