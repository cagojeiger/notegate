import { render, screen, waitFor } from "@testing-library/react";
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
});
