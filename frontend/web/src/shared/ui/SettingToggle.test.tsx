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
});
