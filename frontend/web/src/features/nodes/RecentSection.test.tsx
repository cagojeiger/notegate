import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { NodeSummary } from "../../api/types";
import { makeNodeSummary, makeSpace } from "../../test/fixtures";
import { RecentSection } from "./RecentSection";

const mocks = vi.hoisted(() => ({
  useRecentNodesQuery: vi.fn(),
  fetchNextPage: vi.fn()
}));

vi.mock("./useNodeQueries", () => ({
  useRecentNodesQuery: mocks.useRecentNodesQuery
}));

const space = makeSpace();

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

  it("opens folders from the recent list", () => {
    const folder = node("folder-1", "folder");
    const onOpenNode = vi.fn();
    const onInspectNode = vi.fn();
    mocks.useRecentNodesQuery.mockReturnValue(query([
      page([folder], false, null)
    ]));

    const view = renderRecent(onOpenNode, onInspectNode);

    fireEvent.click(view.getByRole("button", { name: folder.name }));
    expect(onInspectNode).toHaveBeenCalledWith(folder);
    expect(onOpenNode).toHaveBeenCalledWith(folder);
  });

  it("uses the shared sidebar typography and compact row rhythm", () => {
    mocks.useRecentNodesQuery.mockReturnValue(query([
      page([node("node-1"), node("node-2")], false, null)
    ]));

    const view = renderRecent();

    expect(view.container.querySelector("section")).toHaveClass("py-1.5", "font-ui");
    expect(view.container.querySelector("[data-recent-list]")).toHaveClass("mt-0.5");
    expect(view.container.querySelector("[data-recent-list] > div")).toHaveClass("space-y-0.5");
    const recentToggle = view.getByRole("button", { name: "Recent" });
    expect(recentToggle).toHaveClass("font-ui", "text-xs");
    expect(recentToggle.querySelectorAll("svg")).toHaveLength(1);
  });
});

function renderRecent(onOpenNode = vi.fn(), onInspectNode = vi.fn()) {
  return render(
    <RecentSection
      activeSpace={space}
      openedNodeId={null}
      inspectedNodeId={null}
      density="compact"
      open
      onToggle={vi.fn()}
      onToggleDensity={vi.fn()}
      onOpenNode={onOpenNode}
      onInspectNode={onInspectNode}
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

function node(id: string, kind: NodeSummary["kind"] = "text"): NodeSummary {
  return makeNodeSummary({
    id,
    space_id: space.id,
    parent_id: space.root_node_id,
    name: id,
    path: `/${id}`,
    kind
  });
}
