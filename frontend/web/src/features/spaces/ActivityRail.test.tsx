import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { makeSpace } from "../../test/fixtures";
import { ActivityRail } from "./ActivityRail";

const space = makeSpace();

describe("ActivityRail", () => {
  it("marks the selected Space as current in the workbench", () => {
    const view = renderRail(false);

    expect(screen.getByRole("button", { name: "Daily" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Open space library" })).not.toHaveAttribute("aria-current");
    expect(view.container.querySelectorAll("[data-active-indicator]")).toHaveLength(1);
  });

  it("marks only the Library as current while the Library is open", () => {
    const view = renderRail(true);

    const libraryButton = screen.getByRole("button", { name: "Open space library" });
    expect(libraryButton).toHaveAttribute("aria-current", "page");
    expect(libraryButton.parentElement).toHaveClass("h-12");
    expect(screen.getByRole("button", { name: "Daily" })).not.toHaveAttribute("aria-current");
    expect(view.container.querySelectorAll("[data-active-indicator]")).toHaveLength(1);
  });

  it("disables destination changes while navigation is locked", () => {
    renderRail(false, true);

    expect(screen.getByRole("button", { name: "Open space library" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Daily" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "History" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Settings" })).toBeDisabled();
  });
});

function renderRail(libraryActive: boolean, navigationLocked = false) {
  return render(
    <ActivityRail
      spaces={[space]}
      activeSpace={space}
      canCreateSpace
      canManageSpaces
      navigationLocked={navigationLocked}
      onSelectSpace={vi.fn()}
      onReorderSpaces={vi.fn()}
      onCreateSpace={vi.fn()}
      onRenameSpace={vi.fn()}
      onDeleteSpace={vi.fn()}
      onOpenLibrary={vi.fn()}
      libraryActive={libraryActive}
      onOpenHistory={vi.fn()}
      onOpenSettings={vi.fn()}
    />
  );
}
