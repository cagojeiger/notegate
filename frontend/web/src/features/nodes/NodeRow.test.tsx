import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { NodeSummary } from "../../api/types";
import { NodeRow } from "./NodeRow";

const pdf: NodeSummary = {
  id: "pdf-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "report.pdf",
  kind: "file",
  path: "/report.pdf",
  has_children: false,
  effective_write_locked: false,
  updated_at: "2026-07-25T00:00:00Z"
};

describe("NodeRow", () => {
  it("marks an opened file with the shared current indicator", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        selected
        onOpenNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "report.pdf" })).toHaveAttribute("aria-current", "page");
    expect(view.container.querySelector("[data-active-indicator]")).toBeInTheDocument();
  });

  it("keeps an idle file unselected", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        selected={false}
        onOpenNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "report.pdf" })).not.toHaveAttribute("aria-current");
    expect(view.container.querySelector("[data-active-indicator]")).not.toBeInTheDocument();
  });
});
