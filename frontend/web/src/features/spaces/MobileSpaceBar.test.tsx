import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { MobileSpaceBar } from "./MobileSpaceBar";

const spaces: Space[] = [
  {
    id: "space-1",
    name: "Daily",
    sort_order: 0,
    permission: "write",
    root_node_id: "root-1",
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  },
  {
    id: "space-2",
    name: "Work",
    sort_order: 1,
    permission: "write",
    root_node_id: "root-2",
    created_at: "2026-06-13T00:00:00Z",
    updated_at: "2026-06-13T00:00:00Z"
  }
];

describe("MobileSpaceBar", () => {
  it("routes mobile space actions", async () => {
    const user = userEvent.setup();
    const onSelectSpace = vi.fn();
    const onCreateSpace = vi.fn();
    const onOpenHistory = vi.fn();
    const onOpenSettings = vi.fn();

    render(
      <MobileSpaceBar
        spaces={spaces}
        activeSpace={spaces[0]}
        canCreateSpace
        onSelectSpace={onSelectSpace}
        onCreateSpace={onCreateSpace}
        onOpenHistory={onOpenHistory}
        onOpenSettings={onOpenSettings}
      />
    );

    await user.click(screen.getByTitle("Work"));
    await user.click(screen.getByRole("button", { name: "Add space" }));
    await user.click(screen.getByRole("button", { name: "History" }));
    await user.click(screen.getByRole("button", { name: "Settings" }));

    expect(onSelectSpace).toHaveBeenCalledWith(spaces[1]);
    expect(onCreateSpace).toHaveBeenCalledTimes(1);
    expect(onOpenHistory).toHaveBeenCalledTimes(1);
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("accounts for mobile bottom safe area", () => {
    render(
      <MobileSpaceBar
        spaces={spaces}
        activeSpace={spaces[0]}
        canCreateSpace
        onSelectSpace={vi.fn()}
        onCreateSpace={vi.fn()}
        onOpenHistory={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );

    const nav = screen.getByRole("navigation", { name: "Spaces" });
    expect(nav).toHaveClass("h-[calc(3.5rem+env(safe-area-inset-bottom))]");
    expect(nav).toHaveClass("pb-[calc(0.5rem+env(safe-area-inset-bottom))]");
  });
});
