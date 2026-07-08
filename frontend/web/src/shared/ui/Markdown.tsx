import type { Components } from "react-markdown";
import { useEffect, useMemo, useRef, useState, type ComponentProps, type MouseEvent } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { parse as parseYaml } from "yaml";

import { formatCodeBlockLabel } from "../../features/editor/codeBlockLanguage";
import { MermaidBlock } from "../../features/editor/MermaidBlock";
import { ShikiCodeBlock } from "../../features/editor/ShikiCodeBlock";
import { CopyableCodeBlock } from "./CopyableCodeBlock";
import { classifyMarkdownLink, safeMarkdownUrlTransform, type MarkdownImagePolicy, type MarkdownLinkPolicy } from "../lib/markdownLinks";
import { useResetHorizontalScrollOnGrow } from "../../features/editor/useResetHorizontalScrollOnGrow";

type HastElementNode = {
  type?: string;
  tagName?: string;
  properties?: {
    className?: string | string[];
  };
  children?: Array<HastElementNode | { type?: string; value?: string }>;
};

type MarkdownCodeBlock = {
  content: string;
  language: string | null;
};

function createBaseComponents(): Components {
  return {
    table({ children, ...props }) {
      return <TableBlock {...props}>{children}</TableBlock>;
    },
    pre: MarkdownPre
  };
}

function MarkdownPre({ children, node, ...props }: ComponentProps<"pre"> & { node?: unknown }) {
  const codeBlock = readMarkdownCodeBlock(node);

  if (!codeBlock) return <pre {...props}>{children}</pre>;

  const { content, language } = codeBlock;

  if (language?.toLowerCase() === "mermaid") {
    return <MermaidBlock code={content} />;
  }

  const label = language ? formatCodeBlockLabel(language) : "Code";
  const code = language ? <ShikiCodeBlock code={content} language={language} /> : <CodeFallback content={content} />;

  return (
    <CopyableCodeBlock code={content} label={label}>
      {code}
    </CopyableCodeBlock>
  );
}

function readMarkdownCodeBlock(node: unknown): MarkdownCodeBlock | null {
  if (!isHastElement(node) || node.tagName !== "pre") return null;

  const codeNode = node.children?.find((child): child is HastElementNode => isHastElement(child) && child.tagName === "code");
  const textNode = codeNode?.children?.[0];
  const content = textNode && "value" in textNode && typeof textNode.value === "string" ? textNode.value.replace(/\n$/, "") : null;
  if (!codeNode || content === null) return null;

  return {
    content,
    language: readLanguageClass(codeNode.properties?.className)
  };
}

function isHastElement(node: unknown): node is HastElementNode {
  return Boolean(node && typeof node === "object" && (node as HastElementNode).type === "element");
}

function readLanguageClass(className: string | string[] | undefined): string | null {
  const classNames = Array.isArray(className) ? className : typeof className === "string" ? className.split(/\s+/) : [];
  const languageClass = classNames.find((name) => name.startsWith("language-"));
  return languageClass ? languageClass.slice("language-".length) : null;
}

function createComponents(linkPolicy: MarkdownLinkPolicy | undefined, imagePolicy: MarkdownImagePolicy | undefined): Components {
  return {
    ...createBaseComponents(),
    a({ href, children, node: _node, ...props }) {
      const hrefProps = href ? { href } : {};

      function handleClick(event: MouseEvent<HTMLAnchorElement>) {
        if (event.defaultPrevented || !isPlainPrimaryClick(event)) return;
        if (!linkPolicy || !href) return;
        const linkIntent = classifyMarkdownLink(linkPolicy.sourcePath, href);

        if (linkIntent.kind === "internal") {
          event.preventDefault();
          void linkPolicy.onOpenInternalLink(linkIntent.path);
          return;
        }
        if (linkIntent.kind === "invalid") {
          event.preventDefault();
          linkPolicy.onInvalidInternalLink?.();
        }
      }

      return <a {...props} {...hrefProps} onClick={handleClick}>{children}</a>;
    },
    img({ src, alt, node: _node, ...props }) {
      return <MarkdownImage {...props} src={src} alt={alt} imagePolicy={imagePolicy} />;
    }
  };
}

function isPlainPrimaryClick(event: MouseEvent<HTMLAnchorElement>): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}

function TableBlock({ children, ...props }: ComponentProps<"table">) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollOnGrow(scrollRef);

  return (
    <div ref={scrollRef} className="markdown-table-scroll" tabIndex={0}>
      <table {...props}>{children}</table>
    </div>
  );
}

// Renders optional leading YAML frontmatter as preview-only Properties, then
// renders the remaining CommonMark + GitHub-flavored markdown. Raw HTML is
// intentionally not enabled (no rehype-raw), so embedded HTML is escaped.
export function Markdown({ content, linkPolicy, imagePolicy }: { content: string; linkPolicy?: MarkdownLinkPolicy; imagePolicy?: MarkdownImagePolicy }) {
  const document = parseMarkdownDocument(content);
  const components = useMemo(() => createComponents(linkPolicy, imagePolicy), [linkPolicy, imagePolicy]);

  return (
    <div className="markdown">
      <FrontmatterProperties properties={document.frontmatter} />
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components} urlTransform={safeMarkdownUrlTransform}>{document.body}</ReactMarkdown>
    </div>
  );
}

type MarkdownImageState =
  | { status: "idle" }
  | { status: "loading"; path: string }
  | { status: "loaded"; path: string; url: string }
  | { status: "not-found" | "unsupported" | "error"; path: string };

function MarkdownImage({ src, alt, imagePolicy, ...props }: ComponentProps<"img"> & { imagePolicy?: MarkdownImagePolicy }) {
  const [state, setState] = useState<MarkdownImageState>({ status: "idle" });
  const imageIntent = useMemo(() => {
    if (!src || !imagePolicy) return { kind: "external" as const };
    return classifyMarkdownLink(imagePolicy.sourcePath, src);
  }, [imagePolicy, src]);

  useEffect(() => {
    if (imageIntent.kind !== "internal" || !imagePolicy) {
      setState({ status: "idle" });
      return;
    }

    let active = true;
    let objectUrl: string | null = null;
    const path = imageIntent.path;
    setState({ status: "loading", path });

    void imagePolicy.loadInternalImage(path)
      .then((result) => {
        if (!active) return;
        if (result.status === "loaded") {
          objectUrl = URL.createObjectURL(result.blob);
          setState({ status: "loaded", path, url: objectUrl });
          return;
        }
        setState({ status: result.status, path });
      })
      .catch(() => {
        if (active) setState({ status: "error", path });
      });

    return () => {
      active = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [imageIntent, imagePolicy]);

  if (!src) return <ImageFallback alt={alt} message="Image unavailable" />;
  if (!imagePolicy || imageIntent.kind === "external") return <img {...props} src={src} alt={alt ?? ""} loading="lazy" decoding="async" />;
  if (imageIntent.kind === "invalid") return <ImageFallback alt={alt} message="Invalid image link" />;
  if (state.status === "loaded" && state.path === imageIntent.path) return <img {...props} src={state.url} alt={alt ?? ""} loading="lazy" decoding="async" />;
  if (state.status === "not-found" && state.path === imageIntent.path) return <ImageFallback alt={alt} message="Image not found" />;
  if (state.status === "unsupported" && state.path === imageIntent.path) return <ImageFallback alt={alt} message="Image cannot be displayed" />;
  if (state.status === "error" && state.path === imageIntent.path) return <ImageFallback alt={alt} message="Could not load image" />;
  return <ImageFallback alt={alt} message="Loading image..." />;
}

function ImageFallback({ alt, message }: { alt?: string; message: string }) {
  return <span className="markdown-image-fallback">{alt ? `${message}: ${alt}` : message}</span>;
}

function FrontmatterProperties({ properties }: { properties: Record<string, unknown> | null }) {
  const entries = properties ? Object.entries(properties) : [];

  if (entries.length === 0) return null;

  return (
    <section className="markdown-frontmatter" aria-label="Properties">
      <div className="markdown-frontmatter-title">Properties</div>
      <dl className="markdown-frontmatter-list">
        {entries.map(([key, value]) => (
          <div className="markdown-frontmatter-row" key={key}>
            <dt>{key}</dt>
            <dd>{formatFrontmatterValue(value)}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

function parseMarkdownDocument(content: string): { frontmatter: Record<string, unknown> | null; body: string } {
  const firstLineEnd = content.indexOf("\n");
  const firstLine = content.slice(0, firstLineEnd === -1 ? content.length : firstLineEnd).replace(/^\uFEFF/, "").replace(/\r$/, "");

  if (!/^---[ \t]*$/.test(firstLine)) {
    return { frontmatter: null, body: content };
  }

  const frontmatterStart = firstLineEnd === -1 ? content.length : firstLineEnd + 1;
  const closingFence = findFrontmatterClosingFence(content, frontmatterStart);

  if (!closingFence) {
    return { frontmatter: null, body: content };
  }

  const source = content.slice(frontmatterStart, closingFence.start);

  try {
    const parsed = parseYaml(source);

    if (parsed == null) {
      return { frontmatter: {}, body: content.slice(closingFence.end) };
    }

    if (!isPlainRecord(parsed)) {
      return { frontmatter: null, body: content };
    }

    return { frontmatter: parsed, body: content.slice(closingFence.end) };
  } catch {
    return { frontmatter: null, body: content };
  }
}

function findFrontmatterClosingFence(content: string, startIndex: number): { start: number; end: number } | null {
  let index = startIndex;

  while (index < content.length) {
    const lineEnd = content.indexOf("\n", index);
    const lineStop = lineEnd === -1 ? content.length : lineEnd;
    const line = content.slice(index, lineStop).replace(/\r$/, "");

    if (/^(?:---|\.\.\.)[ \t]*$/.test(line)) {
      return { start: index, end: lineEnd === -1 ? content.length : lineEnd + 1 };
    }

    if (lineEnd === -1) break;
    index = lineEnd + 1;
  }

  return null;
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function formatFrontmatterValue(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return value.map(formatFrontmatterValue).join(", ");
  if (typeof value === "object") return JSON.stringify(value) ?? String(value);
  return String(value);
}

function CodeFallback({ content }: { content: string }) {
  const preRef = useRef<HTMLPreElement | null>(null);
  useResetHorizontalScrollOnGrow(preRef);

  return (
    <pre ref={preRef} className="ng-code-fallback" tabIndex={0}>
      <code>{content}</code>
    </pre>
  );
}
