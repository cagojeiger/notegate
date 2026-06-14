import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { TextPreview } from "./TextPreview";

describe("TextPreview", () => {
  it("renders markdown as prose", async () => {
    render(<TextPreview name="note.md" content={"# Hello\n\n- item"} />);
    expect(await screen.findByRole("heading", { name: "Hello" })).toBeInTheDocument();
    expect(screen.getByText("item")).toBeInTheDocument();
  });

  it("renders json as a collapsible tree with a source fallback tab", async () => {
    const user = userEvent.setup();
    render(<TextPreview name="config.json" content={'{"server":{"port":9191}}'} />);

    expect(await screen.findByText(/server/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.getAllByText((_, element) => element?.textContent === '{"server":{"port":9191}}').length).toBeGreaterThan(0);
  });

  it("shows parse errors for invalid structured text", async () => {
    render(<TextPreview name="config.json" content="{" />);
    expect(await screen.findByText(/Could not parse JSON/i)).toBeInTheDocument();
  });
});
