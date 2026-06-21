const SCHEME_PATTERN = /^[A-Za-z][A-Za-z0-9+.-]*:/;
const CONTROL_CHARACTER_PATTERN = /[\u0000-\u001F\u007F]/;
const SAFE_MARKDOWN_URL_PROTOCOLS = new Set(["http", "https", "mailto", "tel"]);

export type MarkdownInternalLinkHandler = (path: string) => void | Promise<void>;
export type MarkdownInvalidInternalLinkHandler = () => void;
export type MarkdownLinkIntent = { kind: "external" } | { kind: "internal"; path: string } | { kind: "invalid" };
export type MarkdownLinkPolicy = {
  sourcePath: string;
  onOpenInternalLink: MarkdownInternalLinkHandler;
  onInvalidInternalLink?: MarkdownInvalidInternalLinkHandler;
};

export function safeMarkdownUrlTransform(value: string): string {
  const colon = value.indexOf(":");
  const questionMark = value.indexOf("?");
  const hash = value.indexOf("#");
  const slash = value.indexOf("/");

  if (
    colon === -1 ||
    (slash !== -1 && colon > slash) ||
    (questionMark !== -1 && colon > questionMark) ||
    (hash !== -1 && colon > hash) ||
    SAFE_MARKDOWN_URL_PROTOCOLS.has(value.slice(0, colon).toLowerCase())
  ) {
    return value;
  }

  return "";
}

export function classifyMarkdownLink(sourcePath: string, href: string): MarkdownLinkIntent {
  if (!isMarkdownNodePathHref(href)) return { kind: "external" };
  const path = markdownLinkToNodePath(sourcePath, href);
  return path ? { kind: "internal", path } : { kind: "invalid" };
}

function markdownLinkToNodePath(sourcePath: string, href: string): string | null {
  const value = href.trim();
  if (!isMarkdownNodePathHref(value)) return null;

  const hashIndex = value.indexOf("#");
  const pathPart = hashIndex === -1 ? value : value.slice(0, hashIndex);
  if (!pathPart || pathPart.includes("?")) return null;

  const decodedPath = decodePathSegments(pathPart);
  if (!decodedPath) return null;

  const absolutePath = decodedPath.startsWith("/") ? decodedPath : joinPath(parentPath(sourcePath), decodedPath);
  return normalizeAbsolutePath(absolutePath);
}

function isMarkdownNodePathHref(href: string): boolean {
  const value = href.trim();
  return Boolean(value) && !value.startsWith("#") && !value.startsWith("//") && !SCHEME_PATTERN.test(value);
}

function decodePathSegments(path: string): string | null {
  const decodedSegments: string[] = [];

  for (const segment of path.split("/")) {
    try {
      const decoded = decodeURIComponent(segment);
      if (decoded.includes("/") || CONTROL_CHARACTER_PATTERN.test(decoded)) return null;
      decodedSegments.push(decoded);
    } catch {
      return null;
    }
  }

  return decodedSegments.join("/");
}

function parentPath(sourcePath: string): string {
  const absolute = sourcePath.startsWith("/") ? sourcePath : `/${sourcePath}`;
  const lastSlash = absolute.lastIndexOf("/");
  if (lastSlash <= 0) return "/";
  return absolute.slice(0, lastSlash);
}

function joinPath(basePath: string, targetPath: string): string {
  return `${basePath === "/" ? "" : basePath}/${targetPath}`;
}

function normalizeAbsolutePath(path: string): string | null {
  const segments: string[] = [];

  for (const segment of path.split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") {
      if (segments.length === 0) return null;
      segments.pop();
      continue;
    }
    segments.push(segment);
  }

  return `/${segments.join("/")}`;
}
