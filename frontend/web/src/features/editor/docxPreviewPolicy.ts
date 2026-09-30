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

export function sanitizeRenderedNode(root: Node) {
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
  for (const attribute of Array.from(element.attributes)) {
    if (attribute.name.toLowerCase().startsWith("on")) element.removeAttribute(attribute.name);
  }

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

export function createFrameCsp(previewDocument: Document) {
  const meta = previewDocument.createElement("meta");
  meta.httpEquiv = "Content-Security-Policy";
  meta.content = DOCX_FRAME_CSP;
  return meta;
}

function isSafeResourceUrl(value: string) {
  return value.startsWith("#")
    || value.startsWith("blob:");
}

export function elementsIn(root: Node): Element[] {
  const elements: Element[] = [];
  if (root.nodeType === Node.ELEMENT_NODE) elements.push(root as Element);
  if (root.nodeType === Node.ELEMENT_NODE || root.nodeType === Node.DOCUMENT_FRAGMENT_NODE) {
    elements.push(...Array.from((root as Element | DocumentFragment).querySelectorAll("*")));
  }
  return elements;
}
