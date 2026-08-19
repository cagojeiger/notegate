import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { queryKeys } from "../../api/queryKeys";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { NodeLinksPanel } from "./NodeLinksPanel";

const intersectionObservers: Array<{
  callback: IntersectionObserverCallback;
  options?: IntersectionObserverInit;
}> = [];

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
    intersectionObservers.length = 0;
    vi.stubGlobal("IntersectionObserver", class {
      constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        intersectionObservers.push({ callback, options });
      }

      disconnect() {}
      observe() {}
      takeRecords() { return []; }
      unobserve() {}
      root = null;
      rootMargin = "";
      thresholds = [];
    });
    Object.values(mocks).forEach((mock) => mock.mockReset());
    mocks.useNodeLinkStatusQuery.mockReturnValue({
      data: {
        status: "idle",
        space_pending: false,
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
    await user.click(screen.getByRole("button", { name: /Links to this document/ }));
    await user.click(screen.getByRole("button", { name: "Open /docs/source.md" }));

    expect(onOpenNode).toHaveBeenNthCalledWith(1, "target-1", "node-1");
    expect(onOpenNode).toHaveBeenNthCalledWith(2, "source-1", "node-1");
    expect(screen.queryByRole("button", { name: "Open /missing.md" })).not.toBeInTheDocument();
  });

  it("loads the next page automatically and requests a manual node sync", async () => {
    const user = userEvent.setup();
    renderPanel();

    expect(intersectionObservers).toHaveLength(1);
    expect(intersectionObservers[0]!.options).toMatchObject({
      root: screen.getByRole("region", { name: "Links from this document" }),
      rootMargin: "160px 0px"
    });
    act(() => {
      intersectionObservers[0]!.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      );
      intersectionObservers[0]!.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      );
    });
    const sync = screen.getByRole("button", { name: "Sync links for note.md" });
    expect(sync).toHaveClass("md:min-h-6", "text-workbench");
    expect(sync).not.toHaveClass("border");
    await user.click(sync);

    expect(mocks.fetchOutgoing).toHaveBeenCalledTimes(1);
    expect(mocks.sync).toHaveBeenCalledWith(expect.objectContaining({ id: "node-1" }));
    expect(screen.queryByRole("button", { name: /load more/i })).not.toBeInTheDocument();
  });

  it("keeps both relation headers visible and expands only the selected list", async () => {
    const user = userEvent.setup();
    mocks.useNodeLinksQuery.mockImplementation((_node, direction: "outgoing" | "incoming") => (
      direction === "outgoing"
        ? linkQuery([
            { node_id: "target-1", path: "/docs/target.md", kind: "link", occurrence_count: 2 }
          ], true, mocks.fetchOutgoing, mocks.refetchOutgoing)
        : linkQuery([
            { node_id: "source-1", path: "/docs/source.md", kind: "image", occurrence_count: 1 }
          ], true, mocks.fetchIncoming, mocks.refetchIncoming)
    ));
    renderPanel();

    const outgoing = screen.getByRole("button", { name: /Links from this document/ });
    const incoming = screen.getByRole("button", { name: /Links to this document/ });
    expect(outgoing).toHaveAttribute("aria-expanded", "true");
    expect(incoming).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("region", { name: "Links from this document" })).toBeVisible();

    await user.click(incoming);
    expect(outgoing).toHaveAttribute("aria-expanded", "false");
    expect(incoming).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: "Links to this document" })).toBeVisible();
    expect(intersectionObservers).toHaveLength(2);

    await user.click(incoming);
    expect(incoming).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("region", { name: "Links to this document" })).not.toBeInTheDocument();
  });

  it("requires an explicit retry after loading another page fails", async () => {
    const user = userEvent.setup();
    mocks.useNodeLinksQuery.mockImplementation((_node, direction: "outgoing" | "incoming") => {
      const query = direction === "outgoing"
        ? linkQuery([
            { node_id: "target-1", path: "/docs/target.md", kind: "link", occurrence_count: 1 }
          ], true, mocks.fetchOutgoing, mocks.refetchOutgoing)
        : linkQuery([], false, mocks.fetchIncoming, mocks.refetchIncoming);
      return direction === "outgoing" ? { ...query, isFetchNextPageError: true } : query;
    });
    renderPanel();

    expect(intersectionObservers).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(mocks.fetchOutgoing).toHaveBeenCalledOnce();
  });

  it("surfaces a failed refetch while keeping cached links visible", async () => {
    const user = userEvent.setup();
    mocks.useNodeLinksQuery.mockImplementation((_node, direction: "outgoing" | "incoming") => {
      const query = direction === "outgoing"
        ? linkQuery([
            { node_id: "target-1", path: "/docs/target.md", kind: "link", occurrence_count: 1 }
          ], false, mocks.fetchOutgoing, mocks.refetchOutgoing)
        : linkQuery([], false, mocks.fetchIncoming, mocks.refetchIncoming);
      return direction === "outgoing" ? { ...query, isError: true } : query;
    });
    renderPanel();

    expect(screen.getByRole("button", { name: "Open /docs/target.md" })).toBeInTheDocument();
    expect(screen.getByText("Could not load links.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(mocks.refetchOutgoing).toHaveBeenCalledOnce();
  });

  it("marks the section count as partial when another page is available", () => {
    mocks.useNodeLinksQuery.mockImplementation((_node, direction: "outgoing" | "incoming") => (
      direction === "outgoing"
        ? pagedLinkQuery([
            [{ node_id: "target-1", path: "/docs/target.md", kind: "link", occurrence_count: 1 }],
            [{ node_id: "target-2", path: "/docs/another.md", kind: "link", occurrence_count: 1 }]
          ], true, mocks.fetchOutgoing, mocks.refetchOutgoing)
        : linkQuery([], false, mocks.fetchIncoming, mocks.refetchIncoming)
    ));
    renderPanel();

    const outgoingSection = screen.getByText("Links from this document").closest("section");
    expect(outgoingSection).not.toBeNull();
    expect(within(outgoingSection!).getByText("2+")).toBeInTheDocument();
    expect(within(outgoingSection!).queryByText("2")).not.toBeInTheDocument();
  });

  it("shows only backlinks for non-text nodes", () => {
    const folder = makeRestNode({ kind: "folder", name: "Docs", path: "/Docs" });
    renderPanel({ node: folder });

    expect(screen.queryByText("Index status")).not.toBeInTheDocument();
    expect(screen.queryByText("Links from this document")).not.toBeInTheDocument();
    expect(screen.getByText("Links to this document")).toBeInTheDocument();
    expect(mocks.useNodeLinkStatusQuery).toHaveBeenCalledWith(folder, true);
  });

  it("keeps backlinks but does not offer outgoing indexing for client-encrypted text", () => {
    const encrypted = makeRestNode({ text_storage_format: "encrypted" });
    renderPanel({ node: encrypted });

    expect(screen.queryByText("Index status")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Sync links for note.md" })
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("Links from client-encrypted text cannot be indexed.")
    ).toBeInTheDocument();
    expect(screen.getByText("Links to this document")).toBeInTheDocument();
    expect(mocks.useNodeLinksQuery).toHaveBeenCalledWith(encrypted, "outgoing", false);
    expect(mocks.useNodeLinksQuery).toHaveBeenCalledWith(encrypted, "incoming", true);
  });

  it("refreshes the selected node links after Space projection work settles", () => {
    const node = makeRestNode();
    mocks.useNodeLinkStatusQuery.mockReturnValue({
      data: {
        status: "idle",
        space_pending: true,
        projected_at: null,
        failure_code: null,
        failed_at: null
      },
      isError: false,
      isLoading: false
    });
    const queryClient = createTestQueryClient();
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const view = renderPanel({ node, queryClient });

    mocks.useNodeLinkStatusQuery.mockReturnValue({
      data: {
        status: "idle",
        space_pending: false,
        projected_at: "2026-08-18T00:00:00Z",
        failure_code: null,
        failed_at: null
      },
      isError: false,
      isLoading: false
    });
    view.rerenderPanel();

    expect(resetQueries).toHaveBeenNthCalledWith(1, {
      queryKey: queryKeys.nodeLinkList(node.space_id, node.id, "outgoing"),
      exact: true
    });
    expect(resetQueries).toHaveBeenNthCalledWith(2, {
      queryKey: queryKeys.nodeLinkList(node.space_id, node.id, "incoming"),
      exact: true
    });
  });

  it("refreshes invalidated links when projection finishes before pending is observed", () => {
    const node = makeRestNode();
    const queryClient = createTestQueryClient();
    const incomingKey = queryKeys.nodeLinkList(node.space_id, node.id, "incoming");
    queryClient.setQueryData(incomingKey, { pages: [], pageParams: [] });
    void queryClient.invalidateQueries({ queryKey: incomingKey, refetchType: "none" });
    const resetQueries = vi.spyOn(queryClient, "resetQueries");

    renderPanel({ node, queryClient });

    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: incomingKey,
      exact: true
    });
  });
});

function renderPanel({
  node = makeRestNode(),
  onOpenNode = vi.fn(),
  queryClient = createTestQueryClient()
}: {
  node?: ReturnType<typeof makeRestNode>;
  onOpenNode?: (nodeId: string, sourceNodeId: string) => void;
  queryClient?: ReturnType<typeof createTestQueryClient>;
} = {}) {
  const renderPanelElement = () => (
    <QueryClientProvider client={queryClient}>
      <NodeLinksPanel node={node} canSync onOpenNode={onOpenNode} />
    </QueryClientProvider>
  );
  const view = render(renderPanelElement());
  return {
    ...view,
    rerenderPanel: () => view.rerender(renderPanelElement())
  };
}

function linkQuery(
  links: Array<{ node_id: string | null; path: string; kind: "link" | "image"; occurrence_count: number }>,
  hasNextPage: boolean,
  fetchNextPage: () => void,
  refetch: () => void
) {
  return pagedLinkQuery([links], hasNextPage, fetchNextPage, refetch);
}

function pagedLinkQuery(
  pages: Array<Array<{ node_id: string | null; path: string; kind: "link" | "image"; occurrence_count: number }>>,
  hasNextPage: boolean,
  fetchNextPage: () => void,
  refetch: () => void
) {
  return {
    data: {
      pages: pages.map((links) => ({
        links,
        page: {
          limit: 50,
          returned: links.length,
          has_more: hasNextPage,
          next_cursor: hasNextPage ? "next" : null
        }
      }))
    },
    fetchNextPage,
    hasNextPage,
    isError: false,
    isFetchNextPageError: false,
    isFetchingNextPage: false,
    isLoading: false,
    refetch
  };
}
