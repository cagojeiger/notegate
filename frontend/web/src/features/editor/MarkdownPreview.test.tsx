import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { MarkdownOutlineProvider, useMarkdownOutlineContext, type MarkdownOutlineIdentity } from "./MarkdownOutlineContext";
import { MarkdownPreview } from "./MarkdownPreview";

const firstDocument: MarkdownOutlineIdentity = { groupId: 3, spaceId: "space-1", nodeId: "document-1" };
const secondDocument: MarkdownOutlineIdentity = { groupId: 3, spaceId: "space-1", nodeId: "document-2" };

describe("MarkdownPreview outline", () => {
  it("derives h1-h4 headings from rendered Markdown without frontmatter or code fences", async () => {
    render(
      <MarkdownOutlineProvider>
        <MarkdownPreview
          outlineIdentity={firstDocument}
          frontmatter={{ title: "# Frontmatter heading" }}
          content={[
            "# 개요",
            "## 반복 제목",
            "## 반복 제목",
            "##### Too deep",
            "```md",
            "# Code heading",
            "```"
          ].join("\n")}
        />
        <OutlineProbe groupId={firstDocument.groupId} />
      </MarkdownOutlineProvider>
    );

    const outline = await screen.findByTestId("outline-probe");
    const items = within(outline).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual(["1:개요", "2:반복 제목", "2:반복 제목"]);
    expect(items.filter((item) => item.getAttribute("data-active") === "true")).toHaveLength(1);
    expect(items[1].getAttribute("data-id")).not.toBe(items[2].getAttribute("data-id"));
  });

  it("restores the last scroll position per document and editor group", async () => {
    const view = render(
      <MarkdownOutlineProvider>
        <MarkdownPreview outlineIdentity={firstDocument} content="# First" />
        <OutlineProbe groupId={firstDocument.groupId} />
      </MarkdownOutlineProvider>
    );
    const scrollRoot = screen.getByTestId("markdown-preview-scroll-region");
    await screen.findByTestId("outline-probe");

    scrollRoot.scrollTop = 117;
    fireEvent.scroll(scrollRoot);
    view.rerender(
      <MarkdownOutlineProvider>
        <MarkdownPreview outlineIdentity={secondDocument} content="# Second" />
        <OutlineProbe groupId={secondDocument.groupId} />
      </MarkdownOutlineProvider>
    );
    await waitFor(() => expect(scrollRoot.scrollTop).toBe(0));

    scrollRoot.scrollTop = 42;
    fireEvent.scroll(scrollRoot);
    view.rerender(
      <MarkdownOutlineProvider>
        <MarkdownPreview outlineIdentity={firstDocument} content="# First" />
        <OutlineProbe groupId={firstDocument.groupId} />
      </MarkdownOutlineProvider>
    );
    await waitFor(() => expect(scrollRoot.scrollTop).toBe(117));
  });

  it("keeps a clicked heading current during smooth scrolling, then settles from the actual position", async () => {
    vi.mocked(window.matchMedia).mockReturnValue({ matches: false } as MediaQueryList);
    render(
      <MarkdownOutlineProvider>
        <MarkdownPreview outlineIdentity={firstDocument} content={["# First", "## Second", "### Third"].join("\n\nBody\n\n")} />
        <OutlineProbe groupId={firstDocument.groupId} />
      </MarkdownOutlineProvider>
    );
    const scrollRoot = screen.getByTestId("markdown-preview-scroll-region");
    const headings = screen.getAllByRole("heading");
    mockScrollGeometry(scrollRoot, headings, [0, 500, 900]);
    const scrollTo = vi.fn();
    Object.defineProperty(scrollRoot, "scrollTo", { configurable: true, value: scrollTo });

    fireEvent.click(screen.getByRole("button", { name: "Navigate to Third" }));

    expect(scrollTo).toHaveBeenCalledWith({ top: 884, behavior: "smooth" });
    expect(activeOutlineItem()).toHaveTextContent("3:Third");

    scrollRoot.scrollTop = 480;
    fireEvent.scroll(scrollRoot);
    expect(activeOutlineItem()).toHaveTextContent("3:Third");

    await waitFor(() => expect(activeOutlineItem()).toHaveTextContent("2:Second"), { timeout: 500 });
  });

  it("uses immediate scrolling when reduced motion is requested", () => {
    vi.mocked(window.matchMedia).mockImplementation((query) => ({ matches: query === "(prefers-reduced-motion: reduce)" }) as MediaQueryList);
    render(
      <MarkdownOutlineProvider>
        <MarkdownPreview outlineIdentity={firstDocument} content={["# First", "## Second"].join("\n\nBody\n\n")} />
        <OutlineProbe groupId={firstDocument.groupId} />
      </MarkdownOutlineProvider>
    );
    const scrollRoot = screen.getByTestId("markdown-preview-scroll-region");
    const headings = screen.getAllByRole("heading");
    mockScrollGeometry(scrollRoot, headings, [0, 500]);
    const scrollTo = vi.fn();
    Object.defineProperty(scrollRoot, "scrollTo", { configurable: true, value: scrollTo });

    fireEvent.click(screen.getByRole("button", { name: "Navigate to Second" }));

    expect(scrollTo).toHaveBeenCalledWith({ top: 484, behavior: "auto" });
  });
});

function OutlineProbe({ groupId }: { groupId: number }) {
  const outline = useMarkdownOutlineContext()?.outlinesByGroup[groupId];
  return (
    <ul data-testid="outline-probe">
      {outline?.items.map((item) => (
        <li key={item.id} data-id={item.id} data-active={item.id === outline.activeItemId ? "true" : "false"}>
          <button type="button" aria-label={`Navigate to ${item.label}`} onClick={() => outline.navigate(item.id)}>
            {item.level}:{item.label}
          </button>
        </li>
      ))}
    </ul>
  );
}

function activeOutlineItem(): HTMLElement {
  const activeItem = within(screen.getByTestId("outline-probe"))
    .getAllByRole("listitem")
    .find((item) => item.getAttribute("data-active") === "true");
  if (!activeItem) throw new Error("Expected an active outline item");
  return activeItem;
}

function mockScrollGeometry(root: HTMLElement, headings: HTMLElement[], headingOffsets: number[]) {
  Object.defineProperties(root, {
    clientHeight: { configurable: true, value: 240 },
    scrollHeight: { configurable: true, value: 1_000 }
  });
  vi.spyOn(root, "getBoundingClientRect").mockImplementation(() => rect(0));
  headings.forEach((heading, index) => {
    vi.spyOn(heading, "getBoundingClientRect").mockImplementation(() => rect(headingOffsets[index] - root.scrollTop));
  });
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
