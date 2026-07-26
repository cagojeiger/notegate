import { render, screen } from "@testing-library/react";
import { useState } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Space } from "../../api/types";
import { SpaceLibrary } from "./SpaceLibrary";

const mocks = vi.hoisted(() => ({
  cardMutate: vi.fn(),
  inspectorMutate: vi.fn(),
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
    navigation_pinned: true,
    user_mcp_enabled: true,
    default_search_enabled: true,
    default_text_encryption_enabled: false,
    features: { text_encryption: true },
    permission: "write",
    root_node_id: "daily-root",
    created_at: "2026-07-01T00:00:00Z",
    updated_at: "2026-07-25T00:00:00Z"
  },
  {
    id: "private",
    name: "Private",
    sort_order: 2000,
    navigation_pinned: false,
    user_mcp_enabled: false,
    default_search_enabled: false,
    default_text_encryption_enabled: false,
    features: { text_encryption: false },
    permission: "write",
    root_node_id: "private-root",
    created_at: "2026-07-02T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  }
];

function renderLibrary(options: {
  isMobile?: boolean;
  inspectorOpen?: boolean;
  onOpenSpace?: (space: Space) => void;
} = {}) {
  const isMobile = options.isMobile ?? false;

  function LibraryHarness() {
    const [inspectorOpen, setInspectorOpen] = useState(
      options.inspectorOpen ?? !isMobile
    );

    return (
      <SpaceLibrary
        spaces={spaces}
        activeSpace={spaces[0]}
        isMobile={isMobile}
        inspectorOpen={inspectorOpen}
        onOpenInspector={() => setInspectorOpen(true)}
        onCloseInspector={() => setInspectorOpen(false)}
        onOpenSpace={options.onOpenSpace ?? vi.fn()}
        onCreateSpace={vi.fn()}
      />
    );
  }

  return render(
    <LibraryHarness />
  );
}

describe("SpaceLibrary", () => {
  beforeEach(() => {
    mocks.cardMutate.mockReset();
    mocks.inspectorMutate.mockReset();
    mocks.reorder.mockReset();
    mocks.useUpdateSpaceMutation.mockImplementation((options?: { silentError?: boolean }) => ({
      mutate: options?.silentError ? mocks.inspectorMutate : mocks.cardMutate,
      isPending: false,
      isError: false
    }));
    mocks.useReorderSpacesMutation.mockReturnValue({
      mutate: mocks.reorder,
      isPending: false
    });
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

  it("shows compact policy states and toggles navigation pin from the card", async () => {
    const user = userEvent.setup();
    renderLibrary();

    expect(screen.getByRole("heading", { name: "Spaces 2" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "All spaces" })).toBeInTheDocument();
    expect(screen.getByTitle("Search default on")).toBeInTheDocument();
    expect(screen.getByTitle("Search default off")).toBeInTheDocument();
    expect(screen.getByTitle("User MCP access on")).toBeInTheDocument();
    expect(screen.getByTitle("User MCP access off")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Pin Private to navigation" }));

    expect(mocks.cardMutate).toHaveBeenCalledWith({
      spaceId: "private",
      navigation_pinned: true
    });
    expect(screen.getByRole("button", { name: "Inspect Daily" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Inspect Private" })).toHaveAttribute("aria-pressed", "false");

    await user.click(screen.getByTitle("Search default off"));
    expect(screen.getByRole("button", { name: "Inspect Private" })).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps access and new-item defaults in the inspector", async () => {
    const user = userEvent.setup();
    renderLibrary();

    await user.click(screen.getByRole("button", { name: "Inspect Private" }));
    expect(screen.getByRole("switch", { name: "User MCP access" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "Include in search" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "Text encryption" })).toBeDisabled();

    await user.click(screen.getByRole("switch", { name: "User MCP access" }));
    await user.click(screen.getByRole("switch", { name: "Include in search" }));

    expect(mocks.inspectorMutate).toHaveBeenNthCalledWith(1, {
      spaceId: "private",
      user_mcp_enabled: true
    });
    expect(mocks.inspectorMutate).toHaveBeenNthCalledWith(2, {
      spaceId: "private",
      default_search_enabled: true
    });
  });

  it("opens the inspector as a mobile sheet only on mobile", async () => {
    const user = userEvent.setup();
    renderLibrary({ isMobile: true });

    expect(screen.queryByText("Space Inspector")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Inspect Private" }));

    expect(screen.getByRole("dialog", { name: "Space Inspector" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog", { name: "Space Inspector" })).not.toBeInTheDocument();
  });

  it("removes the docked inspector from layout when it is closed", () => {
    renderLibrary({ inspectorOpen: false });

    expect(screen.queryByText("Space Inspector")).not.toBeInTheDocument();
  });

  it("offers single-click ordering controls as an alternative to dragging", async () => {
    const user = userEvent.setup();
    renderLibrary();

    expect(screen.getByRole("button", { name: "Move Daily earlier" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Private later" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Move Daily later" }));

    expect(mocks.reorder).toHaveBeenCalledWith({ spaces: [spaces[1], spaces[0]] });
    expect(screen.getByText("Daily moved to position 2 of 2")).toHaveAttribute("role", "status");
  });

  it("opens a space without changing its policies", async () => {
    const user = userEvent.setup();
    const onOpenSpace = vi.fn();
    renderLibrary({ onOpenSpace });

    await user.click(screen.getAllByRole("button", { name: "Open" })[1]);

    expect(onOpenSpace).toHaveBeenCalledWith(spaces[1]);
    expect(mocks.cardMutate).not.toHaveBeenCalled();
    expect(mocks.inspectorMutate).not.toHaveBeenCalled();
    expect(mocks.reorder).not.toHaveBeenCalled();
  });
});
