import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Tabs } from "./Tabs";

const items = [
  { id: "outline", label: "Outline", controls: "outline-panel" },
  { id: "metadata", label: "Metadata", disabled: true },
  { id: "links", label: "Links" },
  { id: "history", label: "History" }
] as const;

function TabsHarness() {
  const [value, setValue] = useState<(typeof items)[number]["id"]>("outline");
  return <Tabs items={[...items]} value={value} onChange={setValue} label="Inspector sections" />;
}

describe("Tabs", () => {
  it("uses roving tab stops and selects tabs while moving focus", async () => {
    const user = userEvent.setup();
    render(<TabsHarness />);

    const outline = screen.getByRole("tab", { name: "Outline" });
    const metadata = screen.getByRole("tab", { name: "Metadata" });
    const links = screen.getByRole("tab", { name: "Links" });
    const history = screen.getByRole("tab", { name: "History" });

    expect(outline).toHaveAttribute("aria-selected", "true");
    expect(outline).toHaveAttribute("id", "outline-panel-tab");
    expect(outline).toHaveAttribute("aria-controls", "outline-panel");
    expect(outline).toHaveAttribute("tabindex", "0");
    expect(links).toHaveAttribute("tabindex", "-1");
    expect(metadata).toBeDisabled();
    expect(metadata).toHaveAttribute("aria-disabled", "true");

    outline.focus();
    await user.keyboard("{ArrowRight}");
    expect(links).toHaveFocus();
    expect(links).toHaveAttribute("aria-selected", "true");
    expect(links).toHaveAttribute("tabindex", "0");
    expect(outline).toHaveAttribute("tabindex", "-1");

    await user.keyboard("{ArrowRight}");
    expect(history).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(outline).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(history).toHaveFocus();
    await user.keyboard("{Home}");
    expect(outline).toHaveFocus();
    await user.keyboard("{End}");
    expect(history).toHaveFocus();
    expect(history).toHaveAttribute("aria-selected", "true");
  });

  it("does not activate a disabled tab", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Tabs items={[...items]} value="outline" onChange={onChange} />);

    await user.click(screen.getByRole("tab", { name: "Metadata" }));

    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps the default appearance and offers flush header tabs", () => {
    const { rerender } = render(<Tabs items={[...items]} value="outline" onChange={() => undefined} />);

    expect(screen.getByRole("tablist")).toHaveClass("mb-5");
    expect(screen.getByRole("tab", { name: "Outline" })).toHaveClass("px-2.5", "py-1.5", "text-workbench");

    rerender(<Tabs items={[...items]} value="outline" onChange={() => undefined} variant="header" />);

    expect(screen.getByRole("tablist")).toHaveClass("h-full", "items-end");
    expect(screen.getByRole("tablist")).not.toHaveClass("mb-5", "border-b");
    expect(screen.getByRole("tab", { name: "Outline" })).toHaveClass(
      "h-[calc(var(--ng-workbench-header-size)-4px)]",
      "px-2",
      "text-workbench"
    );
  });
});
