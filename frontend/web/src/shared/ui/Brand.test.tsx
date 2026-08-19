import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { BrandAppIcon } from "./Brand";

describe("BrandAppIcon", () => {
  it("provides theme-specific app icon assets without changing its footprint", () => {
    const { container } = render(<BrandAppIcon size={24} className="brand-position" />);

    const icon = screen.getByRole("img", { name: "NoteGate" });
    const light = container.querySelector<HTMLImageElement>('img[src="/brand/source/app-icon-light.svg"]');
    const dark = container.querySelector<HTMLImageElement>('img[src="/brand/source/app-icon-dark.svg"]');

    expect(icon).toHaveClass("brand-position");
    expect(icon).toHaveStyle({ width: "24px", height: "24px" });
    expect(light).toHaveClass("ng-brand-theme-light");
    expect(dark).toHaveClass("ng-brand-theme-dark");
    expect(light).toHaveAttribute("width", "24");
    expect(light).toHaveAttribute("height", "24");
    expect(dark).toHaveAttribute("width", "24");
    expect(dark).toHaveAttribute("height", "24");
  });

  it("keeps decorative app icons out of the accessibility tree", () => {
    const { container } = render(<BrandAppIcon decorative />);

    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(container.firstElementChild).toHaveAttribute("aria-hidden", "true");
  });
});
