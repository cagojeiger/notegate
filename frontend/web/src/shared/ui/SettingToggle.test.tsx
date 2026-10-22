import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SettingToggle } from "./SettingToggle";

describe("SettingToggle", () => {
  it("keeps its accessible switch state when activated by keyboard", async () => {
    function Harness() {
      const [checked, setChecked] = useState(false);
      return <SettingToggle icon={null} label="MCP access" checked={checked} disabled={false} onChange={setChecked} />;
    }
    const user = userEvent.setup();
    render(<Harness />);
    const toggle = screen.getByRole("switch", { name: "MCP access" });
    await user.tab();
    expect(toggle).toHaveFocus();
    await user.keyboard(" ");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await user.keyboard("{Enter}");
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("does not activate a disabled switch", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<SettingToggle icon={null} label="MCP access" checked disabled onChange={onChange} />);
    await user.click(screen.getByRole("switch", { name: "MCP access" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps focus while saving and blocks repeated activation", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(<SettingToggle icon={null} label="MCP access" checked disabled={false} onChange={onChange} />);
    const toggle = screen.getByRole("switch", { name: "MCP access" });
    await user.tab();
    await user.keyboard(" ");
    expect(onChange).toHaveBeenCalledExactlyOnceWith(false);

    rerender(<SettingToggle icon={null} label="MCP access" checked disabled={false} pending onChange={onChange} />);
    expect(toggle).toHaveFocus();
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    expect(toggle).toHaveAttribute("aria-busy", "true");
    await user.keyboard(" {Enter}");
    await user.click(toggle);
    expect(onChange).toHaveBeenCalledTimes(1);

    rerender(<SettingToggle icon={null} label="MCP access" checked={false} disabled={false} onChange={onChange} />);
    expect(toggle).toHaveFocus();
    expect(toggle).not.toHaveAttribute("aria-disabled");
    await user.keyboard("{Enter}");
    expect(onChange).toHaveBeenLastCalledWith(true);
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it("does not reclaim focus when saving ends after the user tabs away", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    const content = (pending: boolean) => <>
      <SettingToggle icon={null} label="MCP access" checked disabled={false} pending={pending} onChange={onChange} />
      <button>Next</button>
    </>;
    const { rerender } = render(content(true));
    await user.tab();
    await user.tab();
    expect(screen.getByRole("button", { name: "Next" })).toHaveFocus();
    rerender(content(false));
    expect(screen.getByRole("button", { name: "Next" })).toHaveFocus();
  });
});
