import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useUiStore } from "../../stores/uiStore";
import { MermaidBlock } from "./MermaidBlock";

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(async () => ({ svg: "<svg></svg>" }))
}));

vi.mock("mermaid", () => ({ default: mermaid }));

describe("MermaidBlock", () => {
  beforeEach(() => {
    mermaid.initialize.mockClear();
    mermaid.render.mockClear();
    useUiStore.setState({ theme: "light" });
  });

  it("renders diagrams with the active light or dark theme", async () => {
    render(<MermaidBlock code="graph TD; A-->B" />);

    await waitFor(() => {
      expect(mermaid.initialize).toHaveBeenLastCalledWith(
        expect.objectContaining({ securityLevel: "strict", theme: "neutral" })
      );
    });

    act(() => {
      useUiStore.setState({ theme: "dark" });
    });

    await waitFor(() => {
      expect(mermaid.initialize).toHaveBeenLastCalledWith(
        expect.objectContaining({ securityLevel: "strict", theme: "dark" })
      );
    });
  });
});
