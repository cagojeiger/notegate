import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { HelpTooltip } from "./HelpTooltip";

describe("HelpTooltip", () => {
  it("stays open while hovered and dismisses with Escape", async () => {
    const user = userEvent.setup();
    render(<HelpTooltip label="About search">Search help</HelpTooltip>);

    const trigger = screen.getByRole("button", { name: "About search" });
    const tooltip = screen.getByRole("tooltip", { hidden: true });
    expect(tooltip).not.toBeVisible();

    await user.hover(trigger);
    expect(tooltip).toBeVisible();
    await user.hover(tooltip);
    expect(tooltip).toBeVisible();

    await user.keyboard("{Escape}");
    expect(tooltip).not.toBeVisible();
  });

  it("opens from focus and supports click or tap dismissal", async () => {
    const user = userEvent.setup();
    render(<HelpTooltip label="About access">Access help</HelpTooltip>);

    const trigger = screen.getByRole("button", { name: "About access" });
    const tooltip = screen.getByRole("tooltip", { hidden: true });

    await user.tab();
    expect(trigger).toHaveFocus();
    expect(tooltip).toBeVisible();

    await user.keyboard("{Escape}");
    expect(tooltip).not.toBeVisible();
    await user.click(trigger);
    expect(tooltip).toBeVisible();
    await user.click(trigger);
    expect(tooltip).not.toBeVisible();
  });
});
