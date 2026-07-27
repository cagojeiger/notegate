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
});

function renderRail(libraryActive: boolean) {
  return render(
    <ActivityRail
      spaces={[space]}
      activeSpace={space}
      canCreateSpace
      canManageSpaces
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
