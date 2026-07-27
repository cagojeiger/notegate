import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { makeNodeSummary } from "../../test/fixtures";
import { NodeRow } from "./NodeRow";

const pdf = makeNodeSummary({
  id: "pdf-1",
  name: "report.pdf",
  kind: "file",
  path: "/report.pdf"
});

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

  it("adds a lock badge without replacing the node kind icon", () => {
    const view = render(
      <NodeRow
        node={{ ...pdf, effective_write_locked: true }}
        depth={0}
        selected={false}
        onOpenNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    const openButton = screen.getByRole("button", { name: "report.pdf" });
    expect(openButton).toHaveAccessibleDescription("Write locked");
    expect(view.container.querySelector("[data-node-kind-icon]")).toBeInTheDocument();
    expect(screen.getByTitle("Write locked")).toHaveAttribute("data-node-lock-indicator");
    expect(screen.getByTitle("Write locked")).toHaveClass("text-warning");
  });

  it("does not show a lock badge for a writable node", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        selected={false}
        onOpenNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(view.container.querySelector("[data-node-kind-icon]")).toBeInTheDocument();
    expect(view.container.querySelector("[data-node-lock-indicator]")).not.toBeInTheDocument();
  });
});
