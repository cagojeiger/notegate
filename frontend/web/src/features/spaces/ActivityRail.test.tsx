import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { ActivityRail } from "./ActivityRail";

const spaces: Space[] = [
  {
    id: "space-1",
    name: "Daily",
    sort_order: 0,
    permission: "write",
    root_node_id: "root-1",
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  },
  {
    id: "space-2",
    name: "Design",
    sort_order: 1,
    permission: "write",
    root_node_id: "root-2",
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  }
];

describe("ActivityRail", () => {
  it("exposes each full space name and routes selection", async () => {
    const user = userEvent.setup();
    const onSelectSpace = vi.fn();

    render(
      <ActivityRail
        spaces={spaces}
        activeSpace={spaces[0]}
        canCreateSpace
        canManageSpaces
        onSelectSpace={onSelectSpace}
        onReorderSpaces={vi.fn()}
        onCreateSpace={vi.fn()}
        onRenameSpace={vi.fn()}
        onDeleteSpace={vi.fn()}
        onOpenHistory={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    );

    await user.click(screen.getByRole("button", { name: "Design" }));

    expect(screen.getByRole("button", { name: "Daily" })).toBeInTheDocument();
    expect(onSelectSpace).toHaveBeenCalledWith(spaces[1]);
  });
});
