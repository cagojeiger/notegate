import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AuxiliarySidebar } from "./AuxiliarySidebar";

describe("AuxiliarySidebar", () => {
  it("uses the shared workbench body-header height and seam", () => {
    render(
      <AuxiliarySidebar
        activeNode={null}
        canWriteActiveSpace={false}
        onReplaceMetadata={vi.fn()}
      />
    );

    expect(screen.getByText("Inspector")).toHaveClass("h-12", "border-b", "border-seam");
  });
});
