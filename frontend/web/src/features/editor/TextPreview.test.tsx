import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { copyText } from "../../shared/lib/clipboard";
import { StructuredPreview } from "./StructuredPreview";
import { TextPreview } from "./TextPreview";

vi.mock("../../shared/lib/clipboard", () => ({
  copyText: vi.fn()
}));

describe("TextPreview", () => {
  beforeEach(() => {
    vi.mocked(copyText).mockReset();
    vi.mocked(copyText).mockResolvedValue(true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders markdown as prose", async () => {
    render(<TextPreview name="note.md" content={"# Hello\n\n- item"} />);
    expect(await screen.findByRole("heading", { name: "Hello" })).toBeInTheDocument();
    expect(screen.getByText("item")).toBeInTheDocument();
  });

  it("lets markdown previews use the full editor pane width", async () => {
    const { container } = render(<TextPreview name="matrix.md" content={"| service | note |\n| --- | --- |\n| task_management | long note |"} />);

    const table = await screen.findByRole("table");
    expect(table.closest(".markdown-table-scroll")).toBeInTheDocument();
    const preview = container.querySelector(".markdown")?.parentElement;
    expect(preview).toHaveClass("w-full", "flex-1");
    expect(preview?.className).not.toContain("max-w-");
    expect(preview?.className).not.toContain("mx-auto");
  });

  it("renders markdown frontmatter as properties instead of prose", async () => {
    const { container } = render(
      <TextPreview
        name="terra-client.md"
        content={[
          "---",
          "repo: terra-pdf",
          "category: PDF 처리 · 번역 엔진 (분산)",
          "visibility: internal",
          "tags:",
          "  - pdf",
          "  - translation",
          "---",
          "",
          "# terra-pdf"
        ].join("\n")}
      />
    );

    const properties = await screen.findByRole("region", { name: "Properties" });
    expect(properties).toHaveTextContent("repo");
    expect(properties).toHaveTextContent("terra-pdf");
    expect(properties).toHaveTextContent("category");
    expect(properties).toHaveTextContent("PDF 처리 · 번역 엔진 (분산)");
    expect(properties).toHaveTextContent("tags");
    expect(properties).toHaveTextContent("pdf, translation");
    expect(await screen.findByRole("heading", { name: "terra-pdf" })).toBeInTheDocument();
    expect(container).not.toHaveTextContent("repo: terra-pdf");
    expect(container).not.toHaveTextContent("---");
  });

  it("opens conservative markdown node links through the preview callback", async () => {
    const onOpenInternalLink = vi.fn();
    render(
      <TextPreview
        name="index.md"
        content={"[Access Control](./Policies/Access%20Control%20Policy.md)"}
        markdownLinkPolicy={{
          sourcePath: "/Security and Compliance (원본)/index.md",
          onOpenInternalLink
        }}
      />
    );

    fireEvent.click(await screen.findByRole("link", { name: "Access Control" }));

    expect(onOpenInternalLink).toHaveBeenCalledWith("/Security and Compliance (원본)/Policies/Access Control Policy.md");
  });

  it("does not intercept external markdown links", async () => {
    const onOpenInternalLink = vi.fn();
    render(<TextPreview name="index.md" content={"[External](https://example.com)"} markdownLinkPolicy={{ sourcePath: "/index.md", onOpenInternalLink }} />);
    const link = await screen.findByRole("link", { name: "External" });
    link.addEventListener("click", (event) => event.preventDefault());

    fireEvent.click(link);

    expect(onOpenInternalLink).not.toHaveBeenCalled();
  });

  it("removes unsafe javascript link hrefs", async () => {
    const onOpenInternalLink = vi.fn();
    render(<TextPreview name="index.md" content={"[Unsafe](javascript:alert(1))"} markdownLinkPolicy={{ sourcePath: "/index.md", onOpenInternalLink }} />);
    const link = (await screen.findByText("Unsafe")).closest("a");

    expect(link).not.toHaveAttribute("href");
  });

  it("loads external markdown images only after user confirmation", async () => {
    const user = userEvent.setup();
    const loadInternalImage = vi.fn();
    render(
      <TextPreview
        name="index.md"
        content={"![Logo](https://example.com/logo.png)"}
        markdownImagePolicy={{ sourcePath: "/index.md", loadInternalImage }}
      />
    );

    expect(screen.queryByRole("img", { name: "Logo" })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Load external image: Logo" }));

    const image = await screen.findByRole("img", { name: "Logo" });
    expect(image).toHaveAttribute("src", "https://example.com/logo.png");
    expect(image).toHaveAttribute("referrerpolicy", "no-referrer");
    expect(loadInternalImage).not.toHaveBeenCalled();
  });

  it("requests internal image URLs only when the placeholder nears the viewport", async () => {
    let revealImage = () => {};
    let observedRoot: Element | Document | null | undefined;
    vi.stubGlobal("IntersectionObserver", class {
      constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        observedRoot = options?.root;
        revealImage = () => callback(
          [{ isIntersecting: true } as IntersectionObserverEntry],
          this as unknown as IntersectionObserver
        );
      }

      observe() {}
      disconnect() {}
    });
    const loadInternalImage = vi.fn().mockResolvedValue({ status: "loaded", url: "https://storage.example/logo.png" });
    const { container } = render(
      <TextPreview
        name="index.md"
        content={"![Logo](./Assets/logo.png)"}
        markdownImagePolicy={{ sourcePath: "/Docs/index.md", loadInternalImage }}
      />
    );

    expect(loadInternalImage).not.toHaveBeenCalled();
    expect(observedRoot).toBe(container.querySelector(".overflow-y-auto"));
    act(() => revealImage());

    expect(await screen.findByRole("img", { name: "Logo" })).toHaveAttribute("src", "https://storage.example/logo.png");
    expect(loadInternalImage).toHaveBeenCalledWith("/Docs/Assets/logo.png");
  });

  it("resolves and renders internal markdown image files", async () => {
    const loadInternalImage = vi.fn().mockResolvedValue({ status: "loaded", url: "https://storage.example/logo.png" });
    render(
      <TextPreview
        name="index.md"
        content={"![Logo](./Assets/logo.png)"}
        markdownImagePolicy={{ sourcePath: "/Docs/index.md", loadInternalImage }}
      />
    );

    const image = await screen.findByRole("img", { name: "Logo" });
    expect(image).toHaveAttribute("src", "https://storage.example/logo.png");
    expect(loadInternalImage).toHaveBeenCalledWith("/Docs/Assets/logo.png");
  });

  it("refreshes an internal image URL once after an object load failure", async () => {
    const loadInternalImage = vi.fn()
      .mockResolvedValueOnce({ status: "loaded", url: "https://storage.example/expired.png" })
      .mockResolvedValueOnce({ status: "loaded", url: "https://storage.example/refreshed.png" });
    render(
      <TextPreview
        name="index.md"
        content={"![Logo](./Assets/logo.png)"}
        markdownImagePolicy={{ sourcePath: "/Docs/index.md", loadInternalImage }}
      />
    );

    fireEvent.error(await screen.findByRole("img", { name: "Logo" }));
    const refreshedImage = await screen.findByRole("img", { name: "Logo" });
    await waitFor(() => expect(refreshedImage).toHaveAttribute("src", "https://storage.example/refreshed.png"));
    expect(loadInternalImage).toHaveBeenNthCalledWith(1, "/Docs/Assets/logo.png");
    expect(loadInternalImage).toHaveBeenNthCalledWith(2, "/Docs/Assets/logo.png", { forceRefresh: true });

    fireEvent.error(refreshedImage);
    expect(await screen.findByText("Could not load image: Logo")).toBeInTheDocument();
    expect(loadInternalImage).toHaveBeenCalledTimes(2);
  });

  it("starts a fresh retry budget when the markdown image path changes", async () => {
    let requestNumber = 0;
    const loadInternalImage = vi.fn().mockImplementation(async () => ({
      status: "loaded" as const,
      url: `https://storage.example/image-${++requestNumber}.png`
    }));
    const markdownImagePolicy = { sourcePath: "/Docs/index.md", loadInternalImage };
    const view = render(
      <TextPreview
        name="index.md"
        content={"![Diagram](./Assets/a.png)"}
        markdownImagePolicy={markdownImagePolicy}
      />
    );

    fireEvent.error(await screen.findByRole("img", { name: "Diagram" }));
    await waitFor(() => expect(loadInternalImage).toHaveBeenCalledTimes(2));
    expect(loadInternalImage).toHaveBeenNthCalledWith(2, "/Docs/Assets/a.png", { forceRefresh: true });

    view.rerender(
      <TextPreview
        name="index.md"
        content={"![Diagram](./Assets/b.png)"}
        markdownImagePolicy={markdownImagePolicy}
      />
    );
    await waitFor(() => expect(loadInternalImage).toHaveBeenCalledTimes(3));
    expect(loadInternalImage).toHaveBeenNthCalledWith(3, "/Docs/Assets/b.png");

    view.rerender(
      <TextPreview
        name="index.md"
        content={"![Diagram](./Assets/a.png)"}
        markdownImagePolicy={markdownImagePolicy}
      />
    );
    await waitFor(() => expect(loadInternalImage).toHaveBeenCalledTimes(4));
    expect(loadInternalImage).toHaveBeenNthCalledWith(4, "/Docs/Assets/a.png");
  });

  it("keeps invalid internal-looking markdown images inside the preview", async () => {
    const loadInternalImage = vi.fn();
    render(
      <TextPreview
        name="index.md"
        content={"![Broken](./bad%path.png)"}
        markdownImagePolicy={{ sourcePath: "/index.md", loadInternalImage }}
      />
    );

    expect(await screen.findByText("Invalid image link: Broken")).toBeInTheDocument();
    expect(loadInternalImage).not.toHaveBeenCalled();
  });

  it("shows a placeholder when an internal markdown image cannot be resolved", async () => {
    const loadInternalImage = vi.fn().mockResolvedValue({ status: "not-found" });
    render(
      <TextPreview
        name="index.md"
        content={"![Missing](./missing.png)"}
        markdownImagePolicy={{ sourcePath: "/index.md", loadInternalImage }}
      />
    );

    expect(await screen.findByText("Image not found: Missing")).toBeInTheDocument();
  });

  it("does not render disallowed markdown image protocols", async () => {
    const loadInternalImage = vi.fn();
    render(
      <TextPreview
        name="index.md"
        content={"![Unsafe](javascript:alert(1))\n![Mail](mailto:hello@example.com)"}
        markdownImagePolicy={{ sourcePath: "/index.md", loadInternalImage }}
      />
    );

    expect(await screen.findByText("Image unavailable: Unsafe")).toBeInTheDocument();
    expect(await screen.findByText("Image unavailable: Mail")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Unsafe" })).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Mail" })).not.toBeInTheDocument();
    expect(loadInternalImage).not.toHaveBeenCalled();
  });

  it("reports invalid internal-looking markdown links without opening them", async () => {
    const onOpenInternalLink = vi.fn();
    const onInvalidInternalLink = vi.fn();
    render(<TextPreview name="index.md" content={"[Broken](./bad%path.md)"} markdownLinkPolicy={{ sourcePath: "/index.md", onOpenInternalLink, onInvalidInternalLink }} />);

    fireEvent.click(await screen.findByRole("link", { name: "Broken" }));

    expect(onOpenInternalLink).not.toHaveBeenCalled();
    expect(onInvalidInternalLink).toHaveBeenCalledTimes(1);
  });

  it("keeps invalid internal-looking markdown links inside the app without an invalid handler", async () => {
    render(<TextPreview name="index.md" content={"[Broken](./bad%path.md)"} markdownLinkPolicy={{ sourcePath: "/index.md", onOpenInternalLink: vi.fn() }} />);

    expect(fireEvent.click(await screen.findByRole("link", { name: "Broken" }))).toBe(false);
  });

  it("does not intercept modified or non-primary clicks on markdown node links", async () => {
    const onOpenInternalLink = vi.fn();
    render(<TextPreview name="index.md" content={"[Target](./target.md)"} markdownLinkPolicy={{ sourcePath: "/index.md", onOpenInternalLink }} />);
    const link = await screen.findByRole("link", { name: "Target" });
    const preventNavigation = (event: MouseEvent) => event.preventDefault();

    document.addEventListener("click", preventNavigation);
    try {
      fireEvent.click(link, { ctrlKey: true });
      fireEvent.click(link, { metaKey: true });
      fireEvent.click(link, { shiftKey: true });
      fireEvent.click(link, { altKey: true });
      fireEvent.click(link, { button: 1 });
    } finally {
      document.removeEventListener("click", preventNavigation);
    }

    expect(onOpenInternalLink).not.toHaveBeenCalled();
  });

  it("leaves non-object frontmatter as markdown content", async () => {
    const { container } = render(<TextPreview name="note.md" content={"---\n- one\n---\n\n# Still markdown"} />);

    expect(await screen.findByText("one")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Still markdown" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Properties" })).not.toBeInTheDocument();
    expect(container.querySelectorAll(".markdown hr")).toHaveLength(2);
  });

  it("preserves no-language markdown code blocks", async () => {
    const { container } = render(<TextPreview name="note.md" content={"```\nline 1\nline 2\n```"} />);

    await waitFor(() => expect(container.querySelector("pre.ng-code-fallback")).toBeInTheDocument());
    expect(container.querySelector("pre.ng-code-fallback")?.textContent).toBe("line 1\nline 2");
    expect(screen.getByText("Code")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
  });

  it("copies fenced markdown code block contents", async () => {
    const user = userEvent.setup();
    render(<TextPreview name="note.md" content={"```sh\npnpm web:test\n```"} />);

    await user.click(await screen.findByRole("button", { name: "Copy code" }));

    expect(screen.getByText("Shell")).toBeInTheDocument();
    expect(copyText).toHaveBeenCalledWith("pnpm web:test");
    expect(screen.getByRole("button", { name: "Copied code" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Copied code");
  });

  it("supports markdown code block language ids with symbols", async () => {
    const user = userEvent.setup();
    render(<TextPreview name="note.md" content={"```c++\nint main() {}\n```"} />);

    await user.click(await screen.findByRole("button", { name: "Copy code" }));

    expect(screen.getByText("c++")).toBeInTheDocument();
    expect(copyText).toHaveBeenCalledWith("int main() {}");
  });

  it("adds copy chrome to indented markdown code blocks", async () => {
    const { container } = render(<TextPreview name="note.md" content={"    line 1"} />);

    await waitFor(() => expect(container.querySelector("pre.ng-code-fallback")).toBeInTheDocument());
    expect(container.querySelector("pre.ng-code-fallback")?.textContent).toBe("line 1");
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
    expect(screen.getByText("Code")).toBeInTheDocument();
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
    expect(screen.queryByRole("button", { name: "Copy code" })).not.toBeInTheDocument();
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

  it("resets markdown table horizontal scroll when the pane grows wider", async () => {
    const resizeObserver = installSingleResizeObserverMock();
    const clientWidth = installClientWidthMock();

    try {
      const { container } = render(<TextPreview name="matrix.md" content={"| service | note |\n| --- | --- |\n| task_management | long note |"} />);
      await screen.findByRole("table");
      const tableScroll = container.querySelector(".markdown-table-scroll") as HTMLElement;

      expect(tableScroll).toBeInTheDocument();
      expectScrollResetOnGrow(tableScroll, clientWidth, resizeObserver);
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
