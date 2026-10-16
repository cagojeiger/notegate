import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
    mermaid.render.mockReset().mockResolvedValue({ svg: "<svg></svg>" });
    useUiStore.setState({ theme: "light" });
  });

  it.each([
    "graph TD; A-->B",
    'flowchart LR; A@{ img: "https://example.invalid/image.png" }',
    'flowchart LR; A["<img src=\'https://example.invalid/image.png\'>"]',
    'flowchart LR; A[Text]; style A fill:url(https://example.invalid/image.png)'
  ])("waits for consent before invoking the renderer for %s", async (code) => {
    render(<MermaidBlock code={code} />);
    await act(async () => {});
    expect(screen.getByRole("button", { name: "Load diagram" })).toBeVisible();
    expect(screen.getByText("Diagrams may load images and other resources from external sites.")).toBeVisible();
    expect(mermaid.initialize).not.toHaveBeenCalled();
    expect(mermaid.render).not.toHaveBeenCalled();
  });

  it("renders approved diagrams with the active light or dark theme", async () => {
    render(<MermaidBlock code="graph TD; A-->B" />);
    fireEvent.click(screen.getByRole("button", { name: "Load diagram" }));

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
    expect(screen.queryByRole("button", { name: "Load diagram" })).not.toBeInTheDocument();
  });

  it("requires new consent when code changes, even while an approved render is pending", async () => {
    let complete!: (result: { svg: string }) => void;
    const pending = new Promise<{ svg: string }>((resolve) => { complete = resolve; });
    mermaid.render.mockImplementationOnce(() => pending);
    const { rerender, container } = render(<MermaidBlock code="graph TD; A-->B" />);
    fireEvent.click(screen.getByRole("button", { name: "Load diagram" }));
    await waitFor(() => expect(mermaid.render).toHaveBeenCalledOnce());

    const changed = 'flowchart LR; C@{ img: "https://example.invalid/changed.png" }';
    rerender(<MermaidBlock code={changed} />);
    await act(async () => { complete({ svg: "<svg><text>Old diagram</text></svg>" }); });
    expect(screen.getByRole("button", { name: "Load diagram" })).toBeVisible();
    expect(container.querySelector("svg")).toBeNull();
    expect(mermaid.render).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByRole("button", { name: "Load diagram" }));
    await waitFor(() => expect(mermaid.render).toHaveBeenLastCalledWith(expect.any(String), changed));
  });
});
