import { describe, expect, it } from "vitest";

import { activeMarkdownHeadingId, type CollectedMarkdownHeading } from "./markdownOutline";

describe("activeMarkdownHeadingId", () => {
  it("uses the final heading at the bottom of a scrollable document", () => {
    const root = scrollRoot({ clientHeight: 300, scrollHeight: 1_000, scrollTop: 700 });
    const headings = [heading("first", 0), heading("final", 800)];

    expect(activeMarkdownHeadingId(root, headings)).toBe("final");
  });

  it("does not treat a non-scrollable document as bottomed out", () => {
    const root = scrollRoot({ clientHeight: 300, scrollHeight: 300, scrollTop: 0 });
    const headings = [heading("first", 0), heading("final", 200)];

    expect(activeMarkdownHeadingId(root, headings)).toBe("first");
  });
});

function scrollRoot({ clientHeight, scrollHeight, scrollTop }: { clientHeight: number; scrollHeight: number; scrollTop: number }): HTMLElement {
  const root = document.createElement("div");
  Object.defineProperties(root, {
    clientHeight: { configurable: true, value: clientHeight },
    scrollHeight: { configurable: true, value: scrollHeight },
    scrollTop: { configurable: true, value: scrollTop, writable: true }
  });
  root.getBoundingClientRect = () => rect(0);
  return root;
}

function heading(id: string, top: number): CollectedMarkdownHeading {
  const element = document.createElement("h2");
  element.getBoundingClientRect = () => rect(top);
  return { element, id, label: id, level: 2 };
}

function rect(top: number): DOMRect {
  return {
    bottom: top + 24,
    height: 24,
    left: 0,
    right: 100,
    top,
    width: 100,
    x: 0,
    y: top,
    toJSON: () => ({})
  };
}
