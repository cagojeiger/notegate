import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { NodeLinksPanel } from "./NodeLinksPanel";

const mocks = vi.hoisted(() => ({
  fetchIncoming: vi.fn(),
  fetchOutgoing: vi.fn(),
  refetchIncoming: vi.fn(),
  refetchOutgoing: vi.fn(),
  sync: vi.fn(),
  useNodeLinksQuery: vi.fn(),
  useNodeLinkStatusQuery: vi.fn(),
  useSyncNodeLinksMutation: vi.fn()
}));

vi.mock("./useLinkQueries", () => ({
  useNodeLinksQuery: mocks.useNodeLinksQuery,
  useNodeLinkStatusQuery: mocks.useNodeLinkStatusQuery,
  useSyncNodeLinksMutation: mocks.useSyncNodeLinksMutation
}));

describe("NodeLinksPanel", () => {
  beforeEach(() => {
    Object.values(mocks).forEach((mock) => mock.mockReset());
    mocks.useNodeLinkStatusQuery.mockReturnValue({
      data: {
        status: "idle",
        projected_at: "2026-08-18T00:00:00Z",
        failure_code: null,
        failed_at: null
      },
      isError: false,
      isLoading: false
    });
    mocks.useSyncNodeLinksMutation.mockReturnValue({
      isError: false,
      isPending: false,
      mutate: mocks.sync
    });
    mocks.useNodeLinksQuery.mockImplementation((_node, direction: "outgoing" | "incoming") => (
      direction === "outgoing"
        ? linkQuery([
            { node_id: "target-1", path: "/docs/target.md", kind: "link", occurrence_count: 2 },
            { node_id: null, path: "/missing.md", kind: "link", occurrence_count: 1 }
          ], true, mocks.fetchOutgoing, mocks.refetchOutgoing)
        : linkQuery([
            { node_id: "source-1", path: "/docs/source.md", kind: "image", occurrence_count: 1 }
          ], false, mocks.fetchIncoming, mocks.refetchIncoming)
    ));
  });

  it("shows outgoing, broken, and incoming relationships and opens live targets", async () => {
    const user = userEvent.setup();
    const onOpenNode = vi.fn();
    renderPanel({ onOpenNode });

    expect(screen.getByText("Indexed 2026-08-18")).toBeInTheDocument();
    expect(screen.getByText("×2")).toBeInTheDocument();
    expect(screen.getByText("Broken")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open /docs/target.md" }));
    await user.click(screen.getByRole("button", { name: "Open /docs/source.md" }));

    expect(onOpenNode).toHaveBeenNthCalledWith(1, "target-1");
    expect(onOpenNode).toHaveBeenNthCalledWith(2, "source-1");
    expect(screen.queryByRole("button", { name: "Open /missing.md" })).not.toBeInTheDocument();
  });

  it("loads the next page and requests a manual node sync", async () => {
    const user = userEvent.setup();
    renderPanel();

    const outgoingSection = screen.getByText("Outgoing").closest("section");
    expect(outgoingSection).not.toBeNull();
    await user.click(within(outgoingSection!).getByRole("button", { name: "Load more" }));
    await user.click(screen.getByRole("button", { name: "Sync links for note.md" }));

    expect(mocks.fetchOutgoing).toHaveBeenCalledTimes(1);
    expect(mocks.sync).toHaveBeenCalledWith(expect.objectContaining({ id: "node-1" }));
  });

  it("shows only backlinks for non-text nodes", () => {
    renderPanel({ node: makeRestNode({ kind: "folder", name: "Docs", path: "/Docs" }) });

    expect(screen.queryByText("Index status")).not.toBeInTheDocument();
    expect(screen.queryByText("Outgoing")).not.toBeInTheDocument();
    expect(screen.getByText("Backlinks")).toBeInTheDocument();
  });
});

function renderPanel({
  node = makeRestNode(),
  onOpenNode = vi.fn()
}: {
  node?: ReturnType<typeof makeRestNode>;
  onOpenNode?: (nodeId: string) => void;
} = {}) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <NodeLinksPanel node={node} canSync onOpenNode={onOpenNode} />
    </QueryClientProvider>
  );
}

function linkQuery(
  links: Array<{ node_id: string | null; path: string; kind: "link" | "image"; occurrence_count: number }>,
  hasNextPage: boolean,
  fetchNextPage: () => void,
  refetch: () => void
) {
  return {
    data: {
      pages: [{
        links,
        page: {
          limit: 50,
          returned: links.length,
          has_more: hasNextPage,
          next_cursor: hasNextPage ? "next" : null
        }
      }]
    },
    fetchNextPage,
    hasNextPage,
    isError: false,
    isFetchingNextPage: false,
    isLoading: false,
    refetch
  };
}
