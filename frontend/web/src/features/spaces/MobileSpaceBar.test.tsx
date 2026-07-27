import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { makeSpace } from "../../test/fixtures";
import { MobileSpaceBar } from "./MobileSpaceBar";

const spaces: Space[] = [
  makeSpace(),
  makeSpace({
    id: "space-2",
    name: "Work",
    sort_order: 1,
    root_node_id: "root-2"
  })
];

describe("MobileSpaceBar", () => {
  it("routes mobile space actions", async () => {
    const user = userEvent.setup();
    const onSelectSpace = vi.fn();
    const onCreateSpace = vi.fn();
    const onOpenLibrary = vi.fn();
    const onOpenHistory = vi.fn();
    const onOpenSettings = vi.fn();

    render(
      <MobileSpaceBar
        spaces={spaces}
        activeSpace={spaces[0]}
        canCreateSpace
        onSelectSpace={onSelectSpace}
        onCreateSpace={onCreateSpace}
        onOpenLibrary={onOpenLibrary}
        onOpenHistory={onOpenHistory}
        onOpenSettings={onOpenSettings}
      />
    );

    await user.click(screen.getByTitle("Work"));
    await user.click(screen.getByRole("button", { name: "Open space library" }));
    await user.click(screen.getByRole("button", { name: "Add space" }));
    await user.click(screen.getByRole("button", { name: "History" }));
    await user.click(screen.getByRole("button", { name: "Settings" }));

    expect(onSelectSpace).toHaveBeenCalledWith(spaces[1]);
    expect(onOpenLibrary).toHaveBeenCalledTimes(1);
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

  it("marks only the current mobile destination", () => {
    const view = render(
      <MobileSpaceBar
        spaces={spaces}
        activeSpace={spaces[0]}
        canCreateSpace
        onSelectSpace={vi.fn()}
        onCreateSpace={vi.fn()}
        onOpenLibrary={vi.fn()}
        libraryActive
        onOpenHistory={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "Open space library" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Daily" })).not.toHaveAttribute("aria-current");
    expect(view.container.querySelectorAll("[data-active-indicator]")).toHaveLength(1);
  });

});
