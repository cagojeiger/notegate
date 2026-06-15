import { act, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { describe, expect, it } from "vitest";

import { useResetHorizontalScrollDescendantsOnGrow, useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

describe("useResetHorizontalScrollOnGrow", () => {
  it("resets a direct element's horizontal scroll only when it grows wider", () => {
    const resizeObserver = installResizeObserverMock();
    const clientWidth = installClientWidthMock();

    try {
      render(<DirectScrollBox />);
      const box = screen.getByTestId("scroll-box");

      box.scrollLeft = 120;
      clientWidth.set(box, 240);
      act(() => resizeObserver.trigger(box));
      expect(box.scrollLeft).toBe(120);

      clientWidth.set(box, 480);
      act(() => resizeObserver.trigger(box));
      expect(box.scrollLeft).toBe(0);
    } finally {
      resizeObserver.restore();
      clientWidth.restore();
    }
  });

  it("resets matching descendants when they grow wider", () => {
    const resizeObserver = installResizeObserverMock();
    const clientWidth = installClientWidthMock();

    try {
      render(<DescendantScrollBox />);
      const target = screen.getByTestId("scroll-target");

      target.scrollLeft = 90;
      clientWidth.set(target, 520);
      act(() => resizeObserver.trigger(target));
      expect(target.scrollLeft).toBe(0);
    } finally {
      resizeObserver.restore();
      clientWidth.restore();
    }
  });
});

function DirectScrollBox() {
  const ref = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollOnGrow(ref);

  return <div ref={ref} data-testid="scroll-box" />;
}

function DescendantScrollBox() {
  const ref = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollDescendantsOnGrow(ref, ".scroll-target");

  return (
    <div ref={ref}>
      <pre className="scroll-target" data-testid="scroll-target" />
    </div>
  );
}

function installClientWidthMock() {
  const originalClientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  const widths = new WeakMap<HTMLElement, number>();

  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get() {
      return widths.get(this) ?? 320;
    }
  });

  return {
    set: (element: HTMLElement, width: number) => widths.set(element, width),
    restore: () => {
      if (originalClientWidth) Object.defineProperty(HTMLElement.prototype, "clientWidth", originalClientWidth);
      else delete (HTMLElement.prototype as unknown as { clientWidth?: number }).clientWidth;
    }
  };
}

function installResizeObserverMock() {
  const originalResizeObserver = globalThis.ResizeObserver;
  const callbacks = new Map<Element, ResizeObserverCallback[]>();

  globalThis.ResizeObserver = class {
    private readonly callback: ResizeObserverCallback;
    private readonly targets = new Set<Element>();

    constructor(callback: ResizeObserverCallback) {
      this.callback = callback;
    }

    observe(target: Element) {
      this.targets.add(target);
      callbacks.set(target, [...(callbacks.get(target) ?? []), this.callback]);
    }

    unobserve(target: Element) {
      this.targets.delete(target);
      callbacks.set(target, (callbacks.get(target) ?? []).filter((callback) => callback !== this.callback));
    }

    disconnect() {
      for (const target of this.targets) this.unobserve(target);
    }
  } as typeof ResizeObserver;

  return {
    trigger: (target: Element) => {
      for (const callback of callbacks.get(target) ?? []) callback([], {} as ResizeObserver);
    },
    restore: () => {
      if (originalResizeObserver) globalThis.ResizeObserver = originalResizeObserver;
      else delete (globalThis as unknown as { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;
    }
  };
}
