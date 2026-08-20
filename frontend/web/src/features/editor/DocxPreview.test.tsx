import { render, screen, waitFor } from "@testing-library/react";
import type { HElement, Options } from "docx-preview";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DocxPreview } from "./DocxPreview";

const docxMocks = vi.hoisted(() => ({
  parseAsync: vi.fn(),
  renderDocument: vi.fn(),
  options: undefined as Partial<Options> | undefined
}));

vi.mock("docx-preview", () => {
  function h(input: HElement | Node | string): Node {
    if (typeof input === "string") return document.createTextNode(input);
    if (input instanceof Node) return input;

    const { tagName, className, style, children, ...properties } = input;
    const element = document.createElement(tagName);
    if (className) element.className = className;
    if (typeof style === "string") element.setAttribute("style", style);
    else if (style) Object.assign((element as HTMLElement).style, style);
    for (const [name, value] of Object.entries(properties)) {
      if (value !== undefined) (element as unknown as Record<string, unknown>)[name] = value;
    }
    for (const child of children ?? []) element.appendChild(h(child));
    return element;
  }

  return {
    defaultOptions: { h },
    parseAsync: docxMocks.parseAsync,
    renderDocument: docxMocks.renderDocument
  };
});

describe("DocxPreview", () => {
  beforeEach(() => {
    docxMocks.options = undefined;
    docxMocks.parseAsync.mockReset().mockResolvedValue({ kind: "word-document" });
    docxMocks.renderDocument.mockReset().mockImplementation(async (_document, options: Partial<Options>) => {
      docxMocks.options = options;
      return renderedNodes(options);
    });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(okResponse()));
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  });

  it("renders in a scriptless sandbox with hardened package options and safe links", async () => {
    render(<DocxPreview url="https://storage.example/document.docx" name="document.docx" onError={vi.fn()} />);

    expect(screen.getByRole("status")).toHaveTextContent("Loading DOCX…");
    const frame = screen.getByTitle("document.docx DOCX document") as HTMLIFrameElement;
    expect(frame).toHaveAttribute("sandbox", "allow-same-origin");
    expect(frame).toHaveAttribute("referrerpolicy", "no-referrer");

    await waitFor(() => expect(docxMocks.renderDocument).toHaveBeenCalledTimes(1));

    const preview = screen.getByRole("region", { name: "document.docx DOCX preview" });
    expect(preview).toHaveAttribute("aria-busy", "false");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(vi.mocked(fetch)).toHaveBeenCalledWith(
      "https://storage.example/document.docx",
      expect.objectContaining({
        credentials: "omit",
        referrerPolicy: "no-referrer",
        signal: expect.any(AbortSignal)
      })
    );

    const parseOptions = docxMocks.parseAsync.mock.calls[0][1];
    const renderOptions = docxMocks.renderDocument.mock.calls[0][1];
    expect(parseOptions).toBe(renderOptions);
    expect(renderOptions).toMatchObject({
      className: "ng-docx",
      breakPages: false,
      experimental: false,
      ignoreHeight: true,
      ignoreFonts: true,
      ignoreWidth: true,
      renderAltChunks: false,
      renderChanges: false,
      renderComments: false,
      useBase64URL: false
    });

    const frameDocument = frame.contentDocument!;
    expect(frameDocument.head.querySelector('meta[http-equiv="Content-Security-Policy"]'))
      .toHaveAttribute("content", expect.stringContaining("default-src 'none'"));
    expect(frameDocument.head.querySelector('meta[http-equiv="Content-Security-Policy"]'))
      .toHaveAttribute("content", expect.stringContaining("connect-src 'none'"));
    const layoutStyle = frameDocument.head.querySelector<HTMLStyleElement>(
      "style[data-notegate-docx-layout]"
    );
    expect(layoutStyle?.textContent).toContain("overflow-x: hidden");
    expect(layoutStyle?.textContent).toContain("width: min(100%, 64rem)");
    expect(layoutStyle?.textContent).toContain("box-shadow: none");
    expect(layoutStyle?.textContent).toContain("max-width: 100%");
    const scriptLink = findFrameText(frameDocument, "Script").closest("a")!;
    const dataLink = findFrameText(frameDocument, "Data").closest("a")!;
    const vbscriptLink = findFrameText(frameDocument, "VBScript").closest("a")!;
    const safeLink = findFrameText(frameDocument, "Website").closest("a")!;
    const internalLink = findFrameText(frameDocument, "Section").closest("a")!;
    expect(scriptLink.hasAttribute("href")).toBe(false);
    expect(dataLink.hasAttribute("href")).toBe(false);
    expect(vbscriptLink.hasAttribute("href")).toBe(false);
    expect(safeLink.getAttribute("href")).toBe("https://example.com/path");
    expect(safeLink.getAttribute("target")).toBe("_blank");
    expect(safeLink.getAttribute("rel")).toBe("noopener noreferrer");
    expect(safeLink.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(internalLink.getAttribute("href")).toBe("#section-1");
    expect(internalLink.hasAttribute("target")).toBe(false);
    expect(frameDocument.querySelector("iframe")).toBeNull();
    expect(frameDocument.querySelector("img[alt='External']")?.hasAttribute("src")).toBe(false);
    expect(frameDocument.querySelector("img[alt='Data']")?.hasAttribute("src")).toBe(false);
    expect(frameDocument.querySelector("img[alt='Embedded']")?.getAttribute("src")).toBe("blob:docx-image");
    expect(frameDocument.head.textContent).not.toContain("https://tracker.example");
    expect(frameDocument.head.textContent).toContain("blob:docx-bullet");
    expect(frameDocument.querySelector("[data-escaped-url]")?.hasAttribute("style")).toBe(false);
    expect(frameDocument.querySelector("[data-image-set]")?.hasAttribute("style")).toBe(false);
    expect(document.body).not.toHaveTextContent("Script");
  });

  it("reports a bounded fallback when parsing fails", async () => {
    const onError = vi.fn();
    docxMocks.parseAsync.mockRejectedValue(new Error("invalid package"));

    render(<DocxPreview url="https://storage.example/broken.docx" name="broken.docx" onError={onError} />);

    expect(await screen.findByText("DOCX cannot be displayed")).toBeInTheDocument();
    expect(onError).toHaveBeenCalledTimes(1);
    expect(docxMocks.renderDocument).not.toHaveBeenCalled();
  });

  it("aborts stale loads and revokes embedded object URLs on replacement and unmount", async () => {
    const revokeObjectURL = vi.mocked(URL.revokeObjectURL);
    const view = render(
      <DocxPreview url="https://storage.example/first.docx" name="first.docx" onError={vi.fn()} />
    );

    await waitFor(() => expect(docxMocks.renderDocument).toHaveBeenCalledTimes(1));
    const firstSignal = vi.mocked(fetch).mock.calls[0][1]?.signal;

    view.rerender(
      <DocxPreview url="https://storage.example/second.docx" name="second.docx" onError={vi.fn()} />
    );

    await waitFor(() => expect(docxMocks.renderDocument).toHaveBeenCalledTimes(2));
    expect(firstSignal?.aborted).toBe(true);
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:docx-image");
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:docx-bullet");

    const callsBeforeUnmount = revokeObjectURL.mock.calls.length;
    view.unmount();

    expect(revokeObjectURL.mock.calls.length).toBeGreaterThan(callsBeforeUnmount);
    expect(revokeObjectURL.mock.calls.slice(callsBeforeUnmount).map(([url]) => url)).toEqual(
      expect.arrayContaining(["blob:docx-image", "blob:docx-bullet"])
    );
  });

  it("does not replace the current preview when an aborted render finishes late", async () => {
    let resolveStaleRender: ((nodes: Node[]) => void) | undefined;
    const staleRender = new Promise<Node[]>((resolve) => {
      resolveStaleRender = resolve;
    });
    docxMocks.renderDocument
      .mockImplementationOnce(() => staleRender)
      .mockImplementationOnce(async (_document, options: Partial<Options>) => renderedNodes(options));

    const view = render(
      <DocxPreview url="https://storage.example/stale.docx" name="stale.docx" onError={vi.fn()} />
    );
    await waitFor(() => expect(docxMocks.renderDocument).toHaveBeenCalledTimes(1));
    const staleSignal = vi.mocked(fetch).mock.calls[0][1]?.signal;

    view.rerender(
      <DocxPreview url="https://storage.example/current.docx" name="current.docx" onError={vi.fn()} />
    );
    await waitFor(() => expect(docxMocks.renderDocument).toHaveBeenCalledTimes(2));

    const frame = screen.getByTitle("current.docx DOCX document") as HTMLIFrameElement;
    await waitFor(() => expect(frame.contentDocument?.body).toHaveTextContent("Website"));
    expect(staleSignal?.aborted).toBe(true);

    const staleWrapper = document.createElement("main");
    staleWrapper.textContent = "Stale document";
    const staleImage = document.createElement("img");
    staleImage.src = "blob:stale-docx-image";
    staleWrapper.appendChild(staleImage);
    resolveStaleRender?.([staleWrapper]);

    await waitFor(() => expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:stale-docx-image"));
    expect(frame.contentDocument?.body).toHaveTextContent("Website");
    expect(frame.contentDocument?.body).not.toHaveTextContent("Stale document");
  });
});

function renderedNodes(options: Partial<Options>) {
  const wrapper = document.createElement("main");
  wrapper.className = "ng-docx-wrapper";
  wrapper.append(
    options.h!({ tagName: "a", href: "javascript:alert(1)", children: ["Script"] }),
    options.h!({ tagName: "a", href: "data:text/html,<script>alert(1)</script>", children: ["Data"] }),
    options.h!({ tagName: "a", href: "vbscript:msgbox(1)", children: ["VBScript"] }),
    options.h!({ tagName: "a", href: "https://example.com/path", children: ["Website"] }),
    options.h!({ tagName: "a", href: "#section-1", children: ["Section"] })
  );

  const blockedFrame = document.createElement("iframe");
  blockedFrame.srcdoc = "<script>alert(1)</script>";
  wrapper.appendChild(blockedFrame);

  const externalImage = document.createElement("img");
  externalImage.alt = "External";
  externalImage.src = "https://tracker.example/pixel.png";
  wrapper.appendChild(externalImage);

  const dataImage = document.createElement("img");
  dataImage.alt = "Data";
  dataImage.src = "data:image/png;base64,AA==";
  wrapper.appendChild(dataImage);

  const embeddedImage = document.createElement("img");
  embeddedImage.alt = "Embedded";
  embeddedImage.src = "blob:docx-image";
  wrapper.appendChild(embeddedImage);

  const escapedUrl = document.createElement("div");
  escapedUrl.dataset.escapedUrl = "true";
  escapedUrl.setAttribute(
    "style",
    "background-image:u\\72l(https://tracker.example/escaped.png)"
  );
  wrapper.appendChild(escapedUrl);

  const imageSet = document.createElement("div");
  imageSet.dataset.imageSet = "true";
  imageSet.setAttribute(
    "style",
    "background-image:image-set(url(https://tracker.example/image-set.png) 1x)"
  );
  wrapper.appendChild(imageSet);

  const style = document.createElement("style");
  style.textContent = [
    ".ng-docx-wrapper { background-image: url(https://tracker.example/pixel.png); }",
    ".ng-docx-bullet { background-image: url(blob:docx-bullet); }"
  ].join("\n");
  const escapedStyle = document.createElement("style");
  escapedStyle.textContent =
    ".ng-docx-escaped { background-image: u\\72l(https://tracker.example/escaped-style.png); }";
  return [style, escapedStyle, wrapper];
}

function okResponse(): Response {
  return {
    ok: true,
    arrayBuffer: vi.fn().mockResolvedValue(new ArrayBuffer(8))
  } as unknown as Response;
}

function findFrameText(frameDocument: Document, text: string): HTMLElement {
  const element = Array.from(frameDocument.body.querySelectorAll<HTMLElement>("*"))
    .find((candidate) => candidate.textContent === text);
  if (!element) throw new Error(`Missing frame text: ${text}`);
  return element;
}
