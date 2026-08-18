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
          ...styleNodes.map((node) => previewDocument.adoptNode(node))
        );
        previewDocument.body.replaceChildren(
          ...bodyNodes.map((node) => previewDocument.adoptNode(node))
        );
        setStatus("ready");
      })
      .catch(() => {
        cleanupRenderedPreview(previewDocument, resourceUrls);
        if (!active || controller.signal.aborted) return;
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
        title={`${name} DOCX pages`}
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

function createRenderOptions(resourceUrls: Set<string>): Partial<Options> {
  return {
    className: DOCX_CLASS_NAME,
    experimental: false,
    ignoreFonts: true,
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
  if (inlineStyle) element.setAttribute("style", sanitizeCssText(inlineStyle));
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
  return css
    .replace(/@import\s+[^;]+;?/giu, "")
    .replace(CSS_URL_PATTERN, (match, _quote: string, value: string) => (
      isSafeResourceUrl(value.trim()) ? match : "url(\"\")"
    ));
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
