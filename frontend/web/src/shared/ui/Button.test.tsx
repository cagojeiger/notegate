import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Button } from "./Button";

describe("Button", () => {
  it.each(["xs", "sm", "md"] as const)("keeps %s labels at the workbench font size", (size) => {
    render(<Button size={size}>{size} action</Button>);

    expect(screen.getByRole("button", { name: `${size} action` })).toHaveClass("text-workbench");
  });
});
