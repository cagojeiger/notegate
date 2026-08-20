import { useEffect, useRef, useState } from "react";
import {
  defaultOptions,
  parseAsync,
  renderDocument,
  type HElement,
  type Options
} from "docx-preview";

type PreviewStatus = "loading" | "ready" | "error";

const DOCX_CLASS_NAME = "ng-docx";
const DOCX_FRAME_LAYOUT_CSS = `
html,
body {
  box-sizing: border-box;
  margin: 0;
  min-width: 0;
  background: transparent;
}
body {
  overflow-x: hidden;
}
[data-notegate-docx-flow] {
  box-sizing: border-box;
  display: block;
  width: 100%;
  min-width: 0;
  padding: clamp(12px, 2.5vw, 32px);
  background: transparent;
}
[data-notegate-docx-section] {
  box-sizing: border-box;
  display: block;
  width: 100% !important;
  max-width: 64rem !important;
  min-height: 0 !important;
  margin: 0 auto !important;
  padding: 0 clamp(16px, 4vw, 56px) !important;
  overflow: visible;
  box-shadow: none !important;
}
[data-notegate-docx-section]:first-child {
  padding-top: clamp(24px, 5vw, 64px) !important;
}
[data-notegate-docx-section]:last-child {
  padding-bottom: clamp(24px, 5vw, 64px) !important;
}
[data-notegate-docx-content] {
  min-width: 0;
  max-width: 100%;
  overflow-x: auto;
}
[data-notegate-docx-flow] img,
[data-notegate-docx-flow] svg,
[data-notegate-docx-flow] canvas,
[data-notegate-docx-flow] video {
  max-width: 100% !important;
  height: auto !important;
}
`;
const DOCX_FRAME_CSP = [
  "default-src 'none'",
  "img-src blob:",
  "media-src blob:",
  "font-src blob:",
  "style-src 'unsafe-inline'",
  "connect-src 'none'",
  "frame-src 'none'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'"
].join("; ");
const BLOCKED_ELEMENTS = new Set([
  "base",
  "embed",
  "form",
  "iframe",
  "link",
  "meta",
  "object",
  "script"
]);
const EXTERNAL_LINK_PROTOCOLS = new Set(["http:", "https:", "mailto:"]);
const CSS_URL_PATTERN = /url\(\s*(["']?)([^"')]+)\1\s*\)/giu;
const BLOB_URL_PATTERN = /blob:[^"')\s;]+/giu;

export function DocxPreview({
  url,
  name,
  onError
}: {
  url: string;
  name: string;
  onError: () => void;
}) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const onErrorRef = useRef(onError);
  const [status, setStatus] = useState<PreviewStatus>("loading");

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    const previewDocument = frameRef.current?.contentDocument;
    if (!previewDocument) return;

    const controller = new AbortController();
    const resourceUrls = new Set<string>();
    const renderOptions = createRenderOptions(resourceUrls);
    let active = true;

    cleanupRenderedPreview(previewDocument, resourceUrls);
    setStatus("loading");

    void loadAndRenderDocx(url, controller.signal, renderOptions, resourceUrls)
      .then(({ bodyNodes, styleNodes }) => {
        if (!active) {
          revokeObjectUrls(resourceUrls);
          return;
        }
        previewDocument.head.replaceChildren(
          createFrameCsp(previewDocument),
          ...styleNodes.map((node) => previewDocument.adoptNode(node)),
          createFrameLayoutStyle(previewDocument)
        );
        previewDocument.body.replaceChildren(
          ...bodyNodes.map((node) => previewDocument.adoptNode(node))
        );
        setStatus("ready");
      })
      .catch(() => {
        if (!active || controller.signal.aborted) {
          revokeObjectUrls(resourceUrls);
          return;
        }
        cleanupRenderedPreview(previewDocument, resourceUrls);
        setStatus("error");
        onErrorRef.current();
      });

    return () => {
      active = false;
      controller.abort();
      cleanupRenderedPreview(previewDocument, resourceUrls);
    };
  }, [url]);

  return (
    <div
      data-docx-preview
      className="relative flex min-h-0 flex-1 overflow-hidden bg-bg"
      role="region"
      aria-label={`${name} DOCX preview`}
      aria-busy={status === "loading"}
    >
      {status === "loading" ? <DocxStatus>Loading DOCX…</DocxStatus> : null}
      {status === "error" ? <DocxStatus>DOCX cannot be displayed</DocxStatus> : null}
      <iframe
        ref={frameRef}
        className={status === "ready"
          ? "min-h-0 flex-1 border-0 bg-bg outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
          : "hidden"}
        title={`${name} DOCX document`}
        sandbox="allow-same-origin"
        referrerPolicy="no-referrer"
      />
    </div>
  );
}

async function loadAndRenderDocx(
  url: string,
  signal: AbortSignal,
  options: Partial<Options>,
  resourceUrls: Set<string>
) {
  const response = await fetch(url, {
    credentials: "omit",
    referrerPolicy: "no-referrer",
    signal
  });
  if (!response.ok) throw new Error("DOCX preview request failed");

  const bytes = await response.arrayBuffer();
  if (signal.aborted) throw new DOMException("DOCX preview canceled", "AbortError");
  const document = await parseAsync(bytes, options);
  if (signal.aborted) throw new DOMException("DOCX preview canceled", "AbortError");
  const nodes = await renderDocument(document, options);
  applyContinuousLayoutContract(nodes);

  for (const node of nodes) {
    collectObjectUrls(node, resourceUrls);
    sanitizeRenderedNode(node);
    collectObjectUrls(node, resourceUrls);
  }

  if (signal.aborted) {
    revokeObjectUrls(resourceUrls);
    throw new DOMException("DOCX preview canceled", "AbortError");
  }

  return {
    bodyNodes: nodes.filter((node) => node.nodeName !== "STYLE"),
    styleNodes: nodes.filter((node) => node.nodeName === "STYLE")
  };
}

function applyContinuousLayoutContract(nodes: Node[]) {
  for (const node of nodes) {
    for (const wrapper of elementsIn(node)) {
      if (!wrapper.classList.contains(`${DOCX_CLASS_NAME}-wrapper`)) continue;
      wrapper.setAttribute("data-notegate-docx-flow", "");

      for (const section of Array.from(wrapper.children)) {
        if (!section.matches(`section.${DOCX_CLASS_NAME}`)) continue;
        section.setAttribute("data-notegate-docx-section", "");
        section.querySelector(":scope > article")
          ?.setAttribute("data-notegate-docx-content", "");
      }
    }
  }
}

function createRenderOptions(resourceUrls: Set<string>): Partial<Options> {
  return {
    className: DOCX_CLASS_NAME,
    breakPages: false,
    experimental: false,
    ignoreHeight: true,
    ignoreFonts: true,
    ignoreWidth: true,
    renderAltChunks: false,
    renderChanges: false,
    renderComments: false,
    useBase64URL: false,
    h: (input: HElement | Node | string) => {
      const node = defaultOptions.h(input);
      collectObjectUrls(node, resourceUrls);
      sanitizeRenderedNode(node);
      return node;
    }
  };
}

function sanitizeRenderedNode(root: Node) {
  for (const element of elementsIn(root)) sanitizeElement(element);
}

function sanitizeElement(element: Element) {
  const tagName = element.localName.toLowerCase();
  if (BLOCKED_ELEMENTS.has(tagName)) {
    element.remove();
    return;
  }

  element.removeAttribute("srcdoc");
  element.removeAttribute("srcset");

  if (element instanceof HTMLAnchorElement) {
    sanitizeLink(element);
  } else {
    sanitizeResourceAttribute(element, "href");
    sanitizeResourceAttribute(element, "xlink:href");
  }
  sanitizeResourceAttribute(element, "src");
  sanitizeResourceAttribute(element, "poster");

  if (element instanceof HTMLStyleElement) {
    element.textContent = sanitizeCssText(element.textContent ?? "");
  }
  const inlineStyle = element.getAttribute("style");
  if (inlineStyle) {
    const sanitizedStyle = sanitizeInlineStyle(inlineStyle, element.ownerDocument);
    if (sanitizedStyle) element.setAttribute("style", sanitizedStyle);
    else element.removeAttribute("style");
  }
}

function sanitizeLink(link: HTMLAnchorElement) {
  const href = link.getAttribute("href")?.trim() ?? "";
  link.removeAttribute("download");
  link.removeAttribute("ping");
  link.removeAttribute("referrerpolicy");
  link.removeAttribute("rel");
  link.removeAttribute("target");

  if (href.startsWith("#")) {
    link.setAttribute("href", href);
    return;
  }

  try {
    const parsed = new URL(href);
    if (!EXTERNAL_LINK_PROTOCOLS.has(parsed.protocol)) throw new Error("unsafe protocol");
    link.setAttribute("href", parsed.href);
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noopener noreferrer");
    link.setAttribute("referrerpolicy", "no-referrer");
  } catch {
    link.removeAttribute("href");
  }
}

function sanitizeResourceAttribute(element: Element, attribute: string) {
  const value = element.getAttribute(attribute)?.trim();
  if (value !== undefined && value !== null && !isSafeResourceUrl(value)) {
    element.removeAttribute(attribute);
  }
}

function sanitizeCssText(css: string) {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//gu, "");
  const normalized = decodeCssEscapes(withoutComments);
  if (
    normalized !== withoutComments
    && (/\burl\s*\(/iu.test(normalized) || /@import\b/iu.test(normalized))
  ) return "";

  return withoutComments
    .replace(/@import\s+[^;]+;?/giu, "")
    .replace(CSS_URL_PATTERN, (match, _quote: string, value: string) => (
      isSafeResourceUrl(value.trim()) ? match : "url(\"\")"
    ));
}

function sanitizeInlineStyle(css: string, ownerDocument: Document) {
  const probe = ownerDocument.createElement("span");
  probe.style.cssText = css;
  for (const property of Array.from(probe.style)) {
    const value = probe.style.getPropertyValue(property);
    if (hasUnsafeCssResource(value)) probe.style.removeProperty(property);
  }
  return probe.style.cssText;
}

function hasUnsafeCssResource(value: string) {
  const normalized = decodeCssEscapes(value);
  if (/\b(?:-webkit-)?image-set\s*\(|\bcross-fade\s*\(/iu.test(normalized)) return true;

  let unsafeUrl = false;
  const withoutUrls = normalized.replace(
    CSS_URL_PATTERN,
    (_match, _quote: string, url: string) => {
      if (!isSafeResourceUrl(url.trim())) unsafeUrl = true;
      return "";
    }
  );
  return unsafeUrl || /\burl\s*\(/iu.test(withoutUrls);
}

function decodeCssEscapes(css: string) {
  return css.replace(/\\(?:([\da-f]{1,6})\s?|([^\r\n\f]))/giu, (_match, hex: string, escaped: string) => {
    if (hex) {
      const codePoint = Number.parseInt(hex, 16);
      return codePoint === 0 || codePoint > 0x10ffff ? "\u{fffd}" : String.fromCodePoint(codePoint);
    }
    return escaped;
  });
}

function createFrameCsp(previewDocument: Document) {
  const meta = previewDocument.createElement("meta");
  meta.httpEquiv = "Content-Security-Policy";
  meta.content = DOCX_FRAME_CSP;
  return meta;
}

function createFrameLayoutStyle(previewDocument: Document) {
  const style = previewDocument.createElement("style");
  style.dataset.notegateDocxLayout = "true";
  style.textContent = DOCX_FRAME_LAYOUT_CSS;
  return style;
}

function isSafeResourceUrl(value: string) {
  return value.startsWith("#")
    || value.startsWith("blob:");
}

function elementsIn(root: Node): Element[] {
  const elements: Element[] = [];
  if (root.nodeType === Node.ELEMENT_NODE) elements.push(root as Element);
  if (root.nodeType === Node.ELEMENT_NODE || root.nodeType === Node.DOCUMENT_FRAGMENT_NODE) {
    elements.push(...Array.from((root as Element | DocumentFragment).querySelectorAll("*")));
  }
  return elements;
}

function collectObjectUrls(root: Node, urls: Set<string>) {
  for (const element of elementsIn(root)) {
    for (const attribute of Array.from(element.attributes)) {
      collectBlobMatches(attribute.value, urls);
    }
    if (element instanceof HTMLStyleElement) collectBlobMatches(element.textContent ?? "", urls);
  }
}

function collectBlobMatches(value: string, urls: Set<string>) {
  for (const match of value.matchAll(BLOB_URL_PATTERN)) urls.add(match[0]);
}

function cleanupRenderedPreview(previewDocument: Document, resourceUrls: Set<string>) {
  collectObjectUrls(previewDocument.body, resourceUrls);
  collectObjectUrls(previewDocument.head, resourceUrls);
  previewDocument.body.replaceChildren();
  previewDocument.head.replaceChildren();
  revokeObjectUrls(resourceUrls);
}

function revokeObjectUrls(urls: Set<string>) {
  for (const url of urls) URL.revokeObjectURL(url);
  urls.clear();
}

function DocxStatus({ children }: { children: string }) {
  return <div className="grid min-h-0 flex-1 place-items-center text-sm text-muted" role="status">{children}</div>;
}
