import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StructuredPreview } from "./StructuredPreview";
import { TextPreview } from "./TextPreview";

describe("TextPreview", () => {
  it("renders markdown as prose", async () => {
    render(<TextPreview name="note.md" content={"# Hello\n\n- item"} />);
    expect(await screen.findByRole("heading", { name: "Hello" })).toBeInTheDocument();
    expect(screen.getByText("item")).toBeInTheDocument();
  });

  it("preserves no-language markdown code blocks", async () => {
    const { container } = render(<TextPreview name="note.md" content={"```\nline 1\nline 2\n```"} />);

    await waitFor(() => expect(container.querySelector("pre.ng-code-fallback")).toBeInTheDocument());
    expect(container.querySelector("pre.ng-code-fallback")?.textContent).toBe("line 1\nline 2");
  });

  it("renders plain text without a nested code-block card", () => {
    const { container } = render(<TextPreview name="notes.txt" content={"Just plain text."} />);

    expect(screen.getByText("Just plain text.")).toBeInTheDocument();
    expect(container.querySelector("pre.ng-code-fallback")).not.toBeInTheDocument();
  });

  it("renders json as a collapsible tree", async () => {
    render(<TextPreview name="config.json" content={'{"server":{"port":9191}}'} />);

    expect(await screen.findByRole("tree", { name: "Structured data tree" })).toBeInTheDocument();
    expect(screen.getByText(/server/)).toBeInTheDocument();
    expect(screen.getByText(/port/)).toBeInTheDocument();
  });

  it("renders structured source when controlled by the parent header", async () => {
    render(<StructuredPreview format="json" content={'{"server":{"port":9191}}'} mode="source" />);

    await waitFor(() => expect(screen.getAllByText((_, element) => element?.textContent === '{"server":{"port":9191}}').length).toBeGreaterThan(0));
  });

  it("shows parse errors for invalid structured text", async () => {
    render(<TextPreview name="config.json" content="{" />);
    expect(await screen.findByText(/Could not parse JSON/i)).toBeInTheDocument();
  });

  it("resets preview horizontal scroll positions when panels grow wider", async () => {
    const resizeObserver = installSingleResizeObserverMock();
    const clientWidth = installClientWidthMock();

    try {
      let view = render(<TextPreview name="note.md" content={`\`\`\`\n${"x".repeat(200)}\n\`\`\``} />);
      const markdownCode = await waitFor(() => {
        const pre = view.container.querySelector("pre.ng-code-fallback");
        expect(pre).toBeInTheDocument();
        return pre as HTMLElement;
      });
      expectScrollResetOnGrow(markdownCode, clientWidth, resizeObserver);
      view.unmount();

      view = render(<StructuredPreview format="json" content={`{"${"x".repeat(120)}":"value"}`} />);
      const tree = await screen.findByRole("tree", { name: "Structured data tree" });
      const treeScroll = tree.closest(".overflow-auto");
      expect(treeScroll).toBeInTheDocument();
      expectScrollResetOnGrow(treeScroll as HTMLElement, clientWidth, resizeObserver);
      view.unmount();

      view = render(<TextPreview name="notes.txt" content={"x".repeat(400)} />);
      const plainText = view.container.querySelector("pre");
      expect(plainText).toBeInTheDocument();
      expectScrollResetOnGrow(plainText as HTMLElement, clientWidth, resizeObserver);
    } finally {
      resizeObserver.restore();
      clientWidth.restore();
    }
  });
});

function expectScrollResetOnGrow(element: HTMLElement, clientWidth: ReturnType<typeof installClientWidthMock>, resizeObserver: ReturnType<typeof installSingleResizeObserverMock>) {
  element.scrollLeft = 120;
  clientWidth.set(240);
  act(() => resizeObserver.trigger());
  expect(element.scrollLeft).toBe(120);

  clientWidth.set(480);
  act(() => resizeObserver.trigger());
  expect(element.scrollLeft).toBe(0);
}

function installClientWidthMock() {
  const originalClientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  let width = 320;

  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => width
  });

  return {
    set: (nextWidth: number) => {
      width = nextWidth;
    },
    restore: () => {
      if (originalClientWidth) Object.defineProperty(HTMLElement.prototype, "clientWidth", originalClientWidth);
      else delete (HTMLElement.prototype as unknown as { clientWidth?: number }).clientWidth;
    }
  };
}

function installSingleResizeObserverMock() {
  const originalResizeObserver = globalThis.ResizeObserver;
  let triggerResize: (() => void) | null = null;

  globalThis.ResizeObserver = class {
    constructor(callback: ResizeObserverCallback) {
      triggerResize = () => callback([], this as unknown as ResizeObserver);
    }
    observe() {}
    disconnect() {}
    unobserve() {}
  } as typeof ResizeObserver;

  return {
    trigger: () => triggerResize?.(),
    restore: () => {
      if (originalResizeObserver) globalThis.ResizeObserver = originalResizeObserver;
      else delete (globalThis as unknown as { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;
    }
  };
}
