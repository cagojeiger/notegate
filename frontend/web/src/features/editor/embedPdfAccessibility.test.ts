import { waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { observeEmbedPdfAccessibility } from "./embedPdfAccessibility";

describe("observeEmbedPdfAccessibility", () => {
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

    disconnect();
  });
});
