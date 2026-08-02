import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { makeRestNode } from "../../test/fixtures";
import { NodeLinksSection } from "./NodeLinksSection";

const mocks = vi.hoisted(() => ({
  useNodeLinksQuery: vi.fn()
}));

vi.mock("./useLinkIndexQueries", () => ({
  useNodeLinksQuery: mocks.useNodeLinksQuery
}));

const node = makeRestNode({ id: "source", space_id: "daily", name: "source.md" });

describe("NodeLinksSection", () => {
  beforeEach(() => {
    mocks.useNodeLinksQuery.mockReturnValue({
      data: {
        index: {
          space_id: "daily",
          desired_generation: 4,
          applied_generation: 4,
          status: "ready",
          freshness: "current",
          last_indexed_at: "2026-08-02T00:00:00Z"
        },
        outgoing_count: 2,
        incoming_count: 1,
        broken_count: 1,
        outgoing: [
          {
            id: 1,
            kind: "link",
            status: "resolved",
            raw_href: "./target.md",
            normalized_target_path: "/target.md",
            occurrence_count: 1,
            source_node_id: "source",
            source_name: "source.md",
            source_path: "/source.md",
            target_node_id: "target",
            target_name: "target.md",
            target_path: "/target.md"
          },
          {
            id: 2,
            kind: "image",
            status: "missing",
            raw_href: "./missing.png",
            normalized_target_path: "/missing.png",
            occurrence_count: 1,
            source_node_id: "source",
            source_name: "source.md",
            source_path: "/source.md",
            target_node_id: null,
            target_name: null,
            target_path: null
          }
        ],
        incoming: [
          {
            id: 3,
            kind: "link",
            status: "resolved",
            raw_href: "./source.md",
            normalized_target_path: "/source.md",
            occurrence_count: 1,
            source_node_id: "backlink",
            source_name: "backlink.md",
            source_path: "/backlink.md",
            target_node_id: "source",
            target_name: "source.md",
            target_path: "/source.md"
          }
        ],
        outgoing_truncated: false,
        incoming_truncated: false
      },
      isError: false,
      isLoading: false
    });
  });

  it("shows bounded outgoing, incoming, and broken relations", () => {
    render(<NodeLinksSection node={node} />);

    expect(screen.getByText("2 outgoing")).toBeInTheDocument();
    expect(screen.getByText("1 incoming")).toBeInTheDocument();
    expect(screen.getByText("1 broken")).toBeInTheDocument();
    expect(screen.getByText("target.md")).toBeInTheDocument();
    expect(screen.getByText("/missing.png")).toBeInTheDocument();
    expect(screen.getByText("Missing target")).toBeInTheDocument();
    expect(screen.getByText("backlink.md")).toBeInTheDocument();
  });

  it.each([
    ["rebuilding", "Reindexing links…"],
    ["failed", "The last link-index update failed."]
  ] as const)("does not expose relation rows while the index is %s", (freshness, message) => {
    const current = mocks.useNodeLinksQuery();
    mocks.useNodeLinksQuery.mockReturnValue({
      ...current,
      data: { ...current.data, index: { ...current.data.index, freshness } }
    });

    render(<NodeLinksSection node={node} />);

    expect(screen.getByText(message)).toBeInTheDocument();
    expect(screen.queryByText("target.md")).not.toBeInTheDocument();
  });
});
