import { waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { observeEmbedPdfAccessibility } from "./embedPdfAccessibility";

describe("observeEmbedPdfAccessibility", () => {
  beforeEach(() => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
  });

  afterEach(() => vi.unstubAllGlobals());

  it("repairs the viewer semantics and observes newly rendered pages", async () => {
    const root = document.createElement("div").attachShadow({ mode: "open" });
    root.innerHTML = `
      <div role="tablist"><button>View</button><button></button></div>
      <div style="overflow: auto"><img src="blob:test"></div>
      <input inputmode="numeric">
      <button aria-label="Search"><svg role="img"></svg></button>
    `;

    const disconnect = observeEmbedPdfAccessibility(root);

    expect(root.querySelector('[role="toolbar"]')).toHaveAttribute("aria-label", "PDF viewing mode");
    expect(root.querySelector("button:not([aria-label])")).toHaveTextContent("View");
    expect(root.querySelector("button:empty")).toHaveAttribute("aria-label", "More PDF options");
    expect(root.querySelector("img")).toHaveAttribute("alt", "");
    expect(root.querySelector("img")).toHaveAttribute("role", "presentation");
    expect(root.querySelector('input[inputmode="numeric"]')).toHaveAttribute("aria-label", "Current page");
    expect(root.querySelector('[style*="overflow: auto"]')).toHaveAttribute("tabindex", "0");
    expect(root.querySelector('[style*="overflow: auto"]')).toHaveAttribute("role", "region");
    expect(root.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(root.querySelector("svg")).not.toHaveAttribute("role");

    const nextPage = document.createElement("img");
    nextPage.src = "blob:next";
    root.append(nextPage);
    await waitFor(() => expect(nextPage).toHaveAttribute("role", "presentation"));

    const viewButton = root.querySelector("button");
    if (!viewButton) throw new Error("view button missing");
    viewButton.textContent = "";
    await waitFor(() => expect(viewButton).toHaveAttribute("aria-label", "More PDF options"));

    disconnect();
  });

  it("batches mutations and scans only the added subtrees", async () => {
    const root = document.createElement("div").attachShadow({ mode: "open" });
    const queryAll = vi.spyOn(root, "querySelectorAll");
    const disconnect = observeEmbedPdfAccessibility(root);
    expect(queryAll).toHaveBeenCalledTimes(1);

    const firstPage = document.createElement("div");
    firstPage.innerHTML = '<img src="blob:first">';
    const secondPage = document.createElement("div");
    secondPage.innerHTML = '<button><svg role="img"></svg></button>';
    root.append(firstPage);
    root.append(secondPage);

    await waitFor(() => {
      expect(firstPage.querySelector("img")).toHaveAttribute("role", "presentation");
      expect(secondPage.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    });
    expect(queryAll).toHaveBeenCalledTimes(1);

    disconnect();
  });
});
