import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AuxiliarySidebarFrame, AuxiliarySidebarResizeHandle, PanelOverlay, PrimarySidebarFrame, PrimarySidebarResizeHandle } from "./WorkbenchFrames";
import { WORKBENCH_LAYOUT } from "../shared/model/workbenchLayout";
import { useUiStore } from "../stores/uiStore";

describe("WorkbenchFrames", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("docks the primary sidebar with its current width", () => {
    useUiStore.setState({ primaryWidth: 320 });
    const { container } = render(
      <PrimarySidebarFrame mode="docked">
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
    expect(frame).toHaveClass("fixed", "inset-x-0", "h-[70dvh]");
    expect(screen.getByText("Inspector")).toBeInTheDocument();
  });

  it("does not render hidden panels or resize handles", () => {
    const { container } = render(
      <>
        <PrimarySidebarFrame mode="hidden">
          <div>Files</div>
        </PrimarySidebarFrame>
        <PrimarySidebarResizeHandle visible={false} />
        <AuxiliarySidebarResizeHandle visible={false} />
      </>
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("keeps the resize target wide without adding a second default seam", () => {
    const { container } = render(<PrimarySidebarResizeHandle visible />);

    const handle = container.firstElementChild;
    expect(handle).toHaveClass("w-1", "bg-transparent");
    expect(handle).not.toHaveClass("bg-seam");
    expect(screen.getByRole("separator", { name: "Resize Files sidebar" })).toHaveAttribute(
      "aria-valuenow",
      "300"
    );
  });

  it("uses safe-area offsets for mobile overlays", () => {
    const { container } = render(
      <PrimarySidebarFrame mode="overlay">
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

  it("docks the auxiliary sidebar with the current inspector width", () => {
    useUiStore.setState({ auxiliaryWidth: 380 });
    const { container } = render(
      <AuxiliarySidebarFrame mode="docked">
        <div>Inspector</div>
      </AuxiliarySidebarFrame>
    );

    const frame = container.firstElementChild as HTMLElement;
    expect(frame).toHaveStyle({ width: "380px" });
    expect(frame).toHaveClass("flex", "shrink-0");
  });

  it("exposes a wide accessible resize target for the inspector", () => {
    const { container } = render(<AuxiliarySidebarResizeHandle visible />);

    const handle = container.firstElementChild;
    expect(handle).toHaveClass("w-0", "bg-transparent");
    expect(handle).not.toHaveClass("bg-seam");
    expect(screen.getByRole("separator", { name: "Resize Inspector" })).toHaveAttribute(
      "aria-controls",
      "auxiliary-sidebar-panel"
    );
  });

  it("updates the primary sidebar width while dragging its right edge", () => {
    render(
      <>
        <PrimarySidebarFrame id="primary-sidebar-panel" mode="docked">
          <div>Files</div>
        </PrimarySidebarFrame>
        <PrimarySidebarResizeHandle visible />
      </>
    );

    const separator = screen.getByRole("separator", { name: "Resize Files sidebar" });
    fireEvent.pointerDown(separator, { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 160 });

    expect(document.getElementById("primary-sidebar-panel")).toHaveStyle({ width: "360px" });
    expect(separator).toHaveAttribute("aria-valuenow", "360");
    fireEvent.pointerUp(window);
  });

  it("isolates live Inspector width updates from the panel content", () => {
    const contentRender = vi.fn();
    function InspectorContent() {
      contentRender();
      return <div>Large document outline</div>;
    }

    render(
      <>
        <AuxiliarySidebarResizeHandle visible />
        <AuxiliarySidebarFrame id="auxiliary-sidebar-panel" mode="docked">
          <InspectorContent />
        </AuxiliarySidebarFrame>
      </>
    );

    const separator = screen.getByRole("separator", { name: "Resize Inspector" });
    fireEvent.pointerDown(separator, { clientX: 1000 });
    fireEvent.pointerMove(window, { clientX: 980 });
    fireEvent.pointerMove(window, { clientX: 940 });

    expect(document.getElementById("auxiliary-sidebar-panel")).toHaveStyle({ width: "380px" });
    expect(separator).toHaveAttribute("aria-valuenow", "380");
    expect(contentRender).toHaveBeenCalledTimes(1);
    fireEvent.pointerUp(window);
  });

  it("keeps keyboard Inspector resizing immediate", () => {
    render(
      <>
        <AuxiliarySidebarResizeHandle visible />
        <AuxiliarySidebarFrame id="auxiliary-sidebar-panel" mode="docked">
          <div>Inspector</div>
        </AuxiliarySidebarFrame>
      </>
    );

    const separator = screen.getByRole("separator", { name: "Resize Inspector" });
    fireEvent.keyDown(separator, { key: "ArrowLeft" });

    expect(document.getElementById("auxiliary-sidebar-panel")).toHaveStyle({ width: "330px" });
    expect(separator).toHaveAttribute("aria-valuenow", "330");
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
