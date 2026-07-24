import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Space } from "../entities/space/model";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbench";
import { TitleBar } from "./TitleBar";

const space: Space = {
  id: "space-1",
  name: "Personal",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

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

  it("accounts for mobile top safe area", () => {
    renderTitleBar();

    expect(document.querySelector("header")).toHaveClass("max-md:h-[calc(3rem+env(safe-area-inset-top))]");
    expect(document.querySelector("header")).toHaveClass("max-md:pt-[env(safe-area-inset-top)]");
  });
});
