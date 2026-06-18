import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { AuxiliarySidebarFrame, PanelOverlay, PrimarySidebarFrame, PrimarySidebarResizeHandle } from "./WorkbenchFrames";
import { WORKBENCH_LAYOUT } from "./workbenchLayout";

describe("WorkbenchFrames", () => {
  it("docks the primary sidebar with the user's saved width", () => {
    const { container } = render(
      <PrimarySidebarFrame mode="docked" width={320}>
        <div>Files</div>
      </PrimarySidebarFrame>
    );

    const frame = container.firstElementChild as HTMLElement;
    expect(frame).toHaveStyle({ width: "320px" });
    expect(frame).toHaveClass("flex", "shrink-0");
    expect(screen.getByText("Files")).toBeInTheDocument();
  });

  it("renders floating auxiliary panels without consuming flex width", () => {
    const { container } = render(
      <AuxiliarySidebarFrame mode="overlay">
        <div>Inspector</div>
      </AuxiliarySidebarFrame>
    );

    const frame = container.firstElementChild as HTMLElement;
    expect(frame).toHaveClass("fixed", "inset-x-0", "h-[70vh]");
    expect(screen.getByText("Inspector")).toBeInTheDocument();
  });

  it("does not render hidden panels or resize handles", () => {
    const { container } = render(
      <>
        <PrimarySidebarFrame mode="hidden" width={300}>
          <div>Files</div>
        </PrimarySidebarFrame>
        <PrimarySidebarResizeHandle visible={false} onPointerDown={vi.fn()} />
      </>
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("uses safe-area offsets for mobile overlays", () => {
    const { container } = render(
      <PrimarySidebarFrame mode="overlay" width={300}>
        <div>Files</div>
      </PrimarySidebarFrame>
    );

    const frame = container.firstElementChild as HTMLElement;
    expect(frame).toHaveClass("top-[calc(3rem+env(safe-area-inset-top))]");
    expect(frame).toHaveClass("bottom-[calc(3.5rem+env(safe-area-inset-bottom))]");
    expect(frame).toHaveStyle({
      width: WORKBENCH_LAYOUT.mobilePrimaryWidthPercent,
      maxWidth: `${WORKBENCH_LAYOUT.mobilePrimaryMaxWidth}px`
    });
  });

  it("docks the auxiliary sidebar with the shared inspector width", () => {
    const { container } = render(
      <AuxiliarySidebarFrame mode="docked">
        <div>Inspector</div>
      </AuxiliarySidebarFrame>
    );

    const frame = container.firstElementChild as HTMLElement;
    expect(frame).toHaveStyle({ width: `${WORKBENCH_LAYOUT.auxiliaryWidth}px` });
    expect(frame).toHaveClass("flex", "shrink-0");
  });

  it("closes overlay panels from the backdrop", async () => {
    const onClose = vi.fn();
    render(<PanelOverlay visible onClose={onClose} />);

    const backdrop = screen.getByRole("button", { name: "Close panel" });
    expect(backdrop).toHaveClass("inset-x-0");

    await userEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
