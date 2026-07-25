import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { SpaceLibrary } from "./SpaceLibrary";

const mocks = vi.hoisted(() => ({
  mutate: vi.fn(),
  reorder: vi.fn(),
  useUsageQuery: vi.fn(),
  useReorderSpacesMutation: vi.fn(),
  useUpdateSpaceMutation: vi.fn()
}));

vi.mock("../settings/useUsageQueries", () => ({
  useUsageQuery: mocks.useUsageQuery
}));

vi.mock("./useSpaceQueries", () => ({
  useReorderSpacesMutation: mocks.useReorderSpacesMutation,
  useUpdateSpaceMutation: mocks.useUpdateSpaceMutation
}));

const spaces: Space[] = [
  {
    id: "daily",
    name: "Daily",
    sort_order: 1000,
    pinned: true,
    permission: "write",
    root_node_id: "daily-root",
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-25T00:00:00Z"
  },
  {
    id: "private",
    name: "Private",
    sort_order: 2000,
    pinned: false,
    permission: "write",
    root_node_id: "private-root",
    created_at: "2026-07-02T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  }
];

describe("SpaceLibrary", () => {
  beforeEach(() => {
    mocks.mutate.mockReset();
    mocks.reorder.mockReset();
    mocks.useUpdateSpaceMutation.mockReturnValue({ mutate: mocks.mutate, isPending: false });
    mocks.useReorderSpacesMutation.mockReturnValue({ mutate: mocks.reorder, isPending: false });
    mocks.useUsageQuery.mockReturnValue({
      isLoading: false,
      isError: false,
      data: {
        tier: "free",
        spaces: [
          {
            id: "daily",
            name: "Daily",
            items: { used: 12, limit: 100 },
            text_bytes: { used: 2048, limit: 10240 },
            file_bytes: { used: 4096, limit: 20480 },
            reconciliation_pending: false
          }
        ]
      }
    });
  });

  it("shows one ordered grid while keeping pin as an independent access state", async () => {
    const user = userEvent.setup();
    render(<SpaceLibrary spaces={spaces} activeSpace={spaces[0]} onOpenSpace={vi.fn()} onCreateSpace={vi.fn()} />);

    expect(screen.getByRole("heading", { name: "Spaces 2" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "All spaces" })).toBeInTheDocument();
    expect(screen.getAllByText("12 items · 6 KB").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Inspect Private" }));
    expect(screen.getAllByText("Unpinned").length).toBeGreaterThan(1);

    await user.click(screen.getByRole("button", { name: "Make Private available in user MCP" }));
    expect(mocks.mutate).toHaveBeenCalledWith({ spaceId: "private", pinned: true });
  });

  it("keeps optional guidance behind an accessible help button", async () => {
    const user = userEvent.setup();
    render(<SpaceLibrary spaces={spaces} activeSpace={spaces[0]} onOpenSpace={vi.fn()} onCreateSpace={vi.fn()} />);

    expect(screen.queryByRole("region", { name: "About spaces" })).not.toBeInTheDocument();

    const help = screen.getByRole("button", { name: "About spaces" });
    await user.click(help);

    expect(help).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: "About spaces" })).toHaveTextContent("Pinned spaces are available in your user MCP.");

    await user.keyboard("{Escape}");

    expect(help).toHaveAttribute("aria-expanded", "false");
    expect(help).toHaveFocus();
  });

  it("offers single-click ordering controls as an alternative to dragging", async () => {
    const user = userEvent.setup();
    render(<SpaceLibrary spaces={spaces} activeSpace={spaces[0]} onOpenSpace={vi.fn()} onCreateSpace={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Move Daily earlier" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Private later" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Move Daily later" }));

    expect(mocks.reorder).toHaveBeenCalledWith({ spaces: [spaces[1], spaces[0]] });
  });

  it("opens a space without changing its pin state", async () => {
    const user = userEvent.setup();
    const onOpenSpace = vi.fn();
    render(<SpaceLibrary spaces={spaces} activeSpace={spaces[0]} onOpenSpace={onOpenSpace} onCreateSpace={vi.fn()} />);

    await user.click(screen.getAllByRole("button", { name: "Open" })[1]);

    expect(onOpenSpace).toHaveBeenCalledWith(spaces[1]);
    expect(mocks.mutate).not.toHaveBeenCalled();
    expect(mocks.reorder).not.toHaveBeenCalled();
  });
});
