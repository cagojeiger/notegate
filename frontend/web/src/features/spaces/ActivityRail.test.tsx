import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { ActivityRail } from "./ActivityRail";

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-25T00:00:00Z",
  updated_at: "2026-07-25T00:00:00Z"
};

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
