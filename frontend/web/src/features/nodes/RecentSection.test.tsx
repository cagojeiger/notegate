import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { NodeSummary, Space } from "../../api/types";
import { RecentSection } from "./RecentSection";

const mocks = vi.hoisted(() => ({
  useRecentNodesQuery: vi.fn(),
  fetchNextPage: vi.fn()
}));

vi.mock("./useNodeQueries", () => ({
  useRecentNodesQuery: mocks.useRecentNodesQuery
}));

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

describe("RecentSection", () => {
  beforeEach(() => {
    mocks.fetchNextPage.mockReset();
    vi.stubGlobal("IntersectionObserver", class {
      private readonly callback: IntersectionObserverCallback;

      constructor(callback: IntersectionObserverCallback) {
        this.callback = callback;
      }

      observe() {
        this.callback(
          [{ isIntersecting: true } as IntersectionObserverEntry],
          this as unknown as IntersectionObserver
        );
      }

      disconnect() {}
      unobserve() {}
      takeRecords() { return []; }
      readonly root = null;
      readonly rootMargin = "0px";
      readonly thresholds = [0];
    });
  });

  it("renders later pages and removes boundary duplicates", () => {
    mocks.useRecentNodesQuery.mockReturnValue(query([
      page(Array.from({ length: 50 }, (_, index) => node(`node-${index}`)), true, "next"),
      page([node("node-49"), node("node-50")], false, null)
    ]));

    const view = renderRecent();

    expect(view.container.querySelectorAll("[data-node-row]")).toHaveLength(51);
    expect(view.getByText("node-50")).toBeTruthy();
  });

  it("requests the next cursor once when the load-more row enters view", async () => {
    mocks.useRecentNodesQuery.mockReturnValue(query([
      page([node("node-1")], true, "next")
    ], true));

    renderRecent();

    await waitFor(() => expect(mocks.fetchNextPage).toHaveBeenCalledOnce());
  });

  it("does not mount a load-more trigger after the last page", () => {
    mocks.useRecentNodesQuery.mockReturnValue(query([
      page([node("node-1")], false, null)
    ]));

    const view = renderRecent();

    expect(view.queryByRole("button", { name: /load more/i })).toBeNull();
    expect(mocks.fetchNextPage).not.toHaveBeenCalled();
  });
});

function renderRecent() {
  return render(
    <RecentSection
      activeSpace={space}
      activeNodeId={null}
      density="compact"
      open
      onToggle={vi.fn()}
      onToggleDensity={vi.fn()}
      onOpenNode={vi.fn()}
      onNodeContextMenu={vi.fn()}
    />
  );
}

function query(pages: ReturnType<typeof page>[], hasNextPage = false) {
  return {
    data: { pages, pageParams: pages.map((_page, index) => index === 0 ? null : "next") },
    isLoading: false,
    isError: false,
    hasNextPage,
    isFetchingNextPage: false,
    fetchNextPage: mocks.fetchNextPage
  };
}

function page(nodes: NodeSummary[], hasMore: boolean, nextCursor: string | null) {
  return {
    nodes,
    page: {
      limit: 50,
      returned: nodes.length,
      has_more: hasMore,
      next_cursor: nextCursor
    }
  };
}

function node(id: string): NodeSummary {
  return {
    id,
    space_id: space.id,
    parent_id: space.root_node_id,
    name: id,
    kind: "text",
    path: `/${id}`,
    has_children: false,
    updated_at: "2026-07-25T00:00:00Z"
  };
}
