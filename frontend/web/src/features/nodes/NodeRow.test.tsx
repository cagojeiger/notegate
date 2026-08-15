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
  it("marks an opened file without a decorative side rail", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        inspected={false}
        opened
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "report.pdf" })).toHaveAttribute("aria-current", "page");
    expect(view.container.querySelector("[data-active-indicator]")).not.toBeInTheDocument();
    expect(view.container.querySelector("[data-node-row]")).toHaveClass("bg-[var(--ng-active-surface)]");
  });

  it("keeps an idle file unselected", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        inspected={false}
        opened={false}
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(screen.getByRole("button", { name: "report.pdf" })).not.toHaveAttribute("aria-current");
    expect(view.container.querySelector("[data-active-indicator]")).not.toBeInTheDocument();
  });

  it("uses compact spacing and restrained indentation in the Files tree", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={2}
        inspected={false}
        opened={false}
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(view.container.querySelector("[data-node-row]")).toHaveClass("min-h-tree-row", "font-ui", "text-workbench");
    expect(view.container.querySelector("[data-node-row]")).toHaveStyle({ paddingLeft: "28px" });
  });

  it("keeps Recent metadata secondary in the navigation typeface", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        inspected={false}
        opened={false}
        meta="/report.pdf · 2026-08-05"
        reserveDisclosureSpace={false}
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(view.container.querySelector("[data-node-row]")).toHaveClass("min-h-tree-row", "font-ui", "text-workbench");
    expect(view.container.querySelector("[data-node-row]")).not.toHaveClass("py-0.5");
    expect(view.container.querySelector("[data-node-disclosure-space]")).not.toBeInTheDocument();
    expect(screen.getByText("/report.pdf · 2026-08-05")).toHaveClass("text-[10px]", "leading-[14px]");
  });

  it("highlights the inspected node without marking it as open", () => {
    const view = render(
      <NodeRow
        node={pdf}
        depth={0}
        inspected
        opened={false}
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(view.container.querySelector("[data-node-row]")).toHaveAttribute("data-inspected", "true");
    expect(screen.getByRole("button", { name: "report.pdf" })).not.toHaveAttribute("aria-current");
    expect(view.container.querySelector("[data-active-indicator]")).not.toBeInTheDocument();
  });

  it("adds a lock badge without replacing the node kind icon", () => {
    const view = render(
      <NodeRow
        node={{ ...pdf, effective_write_locked: true }}
        depth={0}
        inspected={false}
        opened={false}
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
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
        inspected={false}
        opened={false}
        onOpenNode={vi.fn()}
        onInspectNode={vi.fn()}
        onNodeContextMenu={vi.fn()}
      />
    );

    expect(view.container.querySelector("[data-node-kind-icon]")).toBeInTheDocument();
    expect(view.container.querySelector("[data-node-lock-indicator]")).not.toBeInTheDocument();
  });
});
