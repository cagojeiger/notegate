import { describe, expect, it } from "vitest";

import type { Space } from "../../api/types";
import { buildSpaceSortOrderUpdates, mergeVisibleSpaceOrder, reorderSpacesByDrop } from "./spaceReorder";

function space(id: string, sort_order = 0): Space {
  return {
    id,
    name: id,
    sort_order,
    pinned: true,
    permission: "write",
    root_node_id: `root-${id}`,
    created_at: "2026-06-14T00:00:00Z",
    updated_at: "2026-06-14T00:00:00Z"
  };
}

describe("spaceReorder", () => {
  it("moves a dragged space before a target", () => {
    const next = reorderSpacesByDrop([space("a"), space("b"), space("c")], "c", "a", "before");
    expect(next.map((item) => item.id)).toEqual(["c", "a", "b"]);
  });

  it("moves a dragged space after a target", () => {
    const next = reorderSpacesByDrop([space("a"), space("b"), space("c")], "a", "c", "after");
    expect(next.map((item) => item.id)).toEqual(["b", "c", "a"]);
  });

  it("keeps the same order for invalid drops", () => {
    const spaces = [space("a"), space("b")];
    expect(reorderSpacesByDrop(spaces, "a", "a", "before")).toBe(spaces);
    expect(reorderSpacesByDrop(spaces, "missing", "a", "before")).toBe(spaces);
  });

  it("builds stable sparse sort_order updates", () => {
    const updates = buildSpaceSortOrderUpdates([space("a", 1000), space("b", 0), space("c", 3000)]);
    expect(updates).toEqual([{ spaceId: "b", sort_order: 2000 }]);
  });

  it("reorders visible spaces without moving hidden spaces out of their slots", () => {
    const first = space("a", 1000);
    const hidden = { ...space("hidden", 2000), pinned: false };
    const last = space("c", 3000);

    const merged = mergeVisibleSpaceOrder([first, hidden, last], [last, first]);

    expect(merged.map((item) => item.id)).toEqual(["c", "hidden", "a"]);
  });
});
