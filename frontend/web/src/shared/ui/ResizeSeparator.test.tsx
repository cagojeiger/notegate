import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ResizeSeparator } from "./ResizeSeparator";

describe("ResizeSeparator", () => {
  it("adjusts a horizontal separator with vertical arrow keys", async () => {
    const user = userEvent.setup();
    const onValueChange = vi.fn();
    render(
      <ResizeSeparator
        orientation="horizontal"
        label="Resize Files section"
        value={67}
        min={20}
        max={82}
        step={5}
        onPointerDown={vi.fn()}
        onValueChange={onValueChange}
      />
    );

    const separator = screen.getByRole("separator", { name: "Resize Files section" });
    expect(separator).toHaveAttribute("aria-valuenow", "67");
    await separator.focus();
    await user.keyboard("{ArrowUp}{ArrowDown}{Home}{End}");

    expect(onValueChange).toHaveBeenNthCalledWith(1, 62);
    expect(onValueChange).toHaveBeenNthCalledWith(2, 72);
    expect(onValueChange).toHaveBeenNthCalledWith(3, 20);
    expect(onValueChange).toHaveBeenNthCalledWith(4, 82);
  });

  it("adjusts a vertical separator with horizontal arrow keys", async () => {
    const user = userEvent.setup();
    const onValueChange = vi.fn();
    render(
      <ResizeSeparator
        orientation="vertical"
        label="Resize Files sidebar"
        value={300}
        min={220}
        max={520}
        step={10}
        onPointerDown={vi.fn()}
        onValueChange={onValueChange}
      />
    );

    const separator = screen.getByRole("separator", { name: "Resize Files sidebar" });
    await separator.focus();
    await user.keyboard("{ArrowLeft}{ArrowRight}");

    expect(onValueChange).toHaveBeenNthCalledWith(1, 290);
    expect(onValueChange).toHaveBeenNthCalledWith(2, 310);
  });
});
