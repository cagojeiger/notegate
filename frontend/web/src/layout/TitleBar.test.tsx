import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";
import { makeSpace } from "../test/fixtures";
import { TitleBar } from "./TitleBar";

const space = makeSpace({
  name: "Personal",
});

function renderTitleBar(overrides: Partial<Parameters<typeof TitleBar>[0]> = {}) {
  const props = {
    activeSpace: space,
    theme: "light" as const,
    primarySidebarOpen: true,
    auxiliaryOpen: true,
    editorGroupCount: 1,
    onAddGroup: vi.fn(),
    onToggleTheme: vi.fn(),
    onTogglePrimarySidebar: vi.fn(),
    onToggleAuxiliary: vi.fn(),
    ...overrides
  };
  render(<TitleBar {...props} />);
  return props;
}

describe("TitleBar", () => {
  it("shows the active space and routes control clicks", async () => {
    const user = userEvent.setup();
    const props = renderTitleBar();

    expect(screen.getByText("NoteGate")).toBeInTheDocument();
    expect(screen.getByText("/ Personal")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Toggle left sidebar" }));
    await user.click(screen.getByRole("button", { name: "Toggle right sidebar" }));
    await user.click(screen.getByRole("button", { name: "Toggle theme" }));
    await user.click(screen.getByRole("button", { name: "Split editor (1/3)" }));

    expect(props.onTogglePrimarySidebar).toHaveBeenCalledTimes(1);
    expect(props.onToggleAuxiliary).toHaveBeenCalledTimes(1);
    expect(props.onToggleTheme).toHaveBeenCalledTimes(1);
    expect(props.onAddGroup).toHaveBeenCalledTimes(1);
  });

  it("disables split at the editor group maximum", () => {
    const props = renderTitleBar({ editorGroupCount: MAX_EDITOR_GROUPS });
    const split = screen.getByRole("button", { name: "Maximum 3 editor groups" });

    expect(split).toBeDisabled();
    expect(props.onAddGroup).not.toHaveBeenCalled();
  });

  it("keeps the inspector toggle when workbench-only controls are hidden", () => {
    renderTitleBar({
      showWorkbenchControls: false,
      auxiliaryLabel: "Toggle space inspector"
    });

    expect(screen.queryByRole("button", { name: "Toggle left sidebar" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Split editor/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Toggle space inspector" })).toBeInTheDocument();
  });

  it("accounts for mobile top safe area", () => {
    renderTitleBar();

    expect(document.querySelector("header")).toHaveClass("max-md:h-[calc(var(--ng-workbench-header-size)+env(safe-area-inset-top))]");
    expect(document.querySelector("header")).toHaveClass("max-md:pt-[env(safe-area-inset-top)]");
  });

  it("aligns the desktop brand with the activity rail grid", () => {
    renderTitleBar();

    expect(document.querySelector("header")).toHaveClass(
      "grid-cols-[44px_minmax(0,1fr)_auto]"
    );
  });
});
