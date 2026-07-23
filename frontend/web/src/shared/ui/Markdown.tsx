import type { Components } from "react-markdown";
import { useEffect, useMemo, useRef, useState, type ComponentProps, type MouseEvent, type Ref, type RefObject } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { formatCodeBlockLabel } from "../../features/editor/codeBlockLanguage";
import { MermaidBlock } from "../../features/editor/MermaidBlock";
import { ShikiCodeBlock } from "../../features/editor/ShikiCodeBlock";
import { CopyableCodeBlock } from "./CopyableCodeBlock";
import { formatFrontmatterValue, parseMarkdownDocument } from "../lib/markdownDocument";
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

function createComponents(
  linkPolicy: MarkdownLinkPolicy | undefined,
  imagePolicy: MarkdownImagePolicy | undefined,
  imageViewportRoot: RefObject<Element | null> | undefined
): Components {
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
      const imageKey = `${imagePolicy?.sourcePath ?? ""}:${src ?? ""}`;
      return <MarkdownImage key={imageKey} {...props} src={src} alt={alt} imagePolicy={imagePolicy} viewportRoot={imageViewportRoot} />;
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
export function Markdown({
  content,
  linkPolicy,
  imagePolicy,
  imageViewportRoot
}: {
  content: string;
  linkPolicy?: MarkdownLinkPolicy;
  imagePolicy?: MarkdownImagePolicy;
  imageViewportRoot?: RefObject<Element | null>;
}) {
  const document = parseMarkdownDocument(content);
  const components = useMemo(
    () => createComponents(linkPolicy, imagePolicy, imageViewportRoot),
    [imagePolicy, imageViewportRoot, linkPolicy]
  );

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

function MarkdownImage({
  src,
  alt,
  imagePolicy,
  viewportRoot,
  ...props
}: ComponentProps<"img"> & {
  imagePolicy?: MarkdownImagePolicy;
  viewportRoot?: RefObject<Element | null>;
}) {
  const [state, setState] = useState<MarkdownImageState>({ status: "idle" });
  const [nearViewportPath, setNearViewportPath] = useState<string | null>(null);
  const [retriedPath, setRetriedPath] = useState<string | null>(null);
  const placeholderRef = useRef<HTMLSpanElement | null>(null);
  const imageIntent = useMemo(() => {
    if (!src) return { kind: "invalid" as const };
    return classifyMarkdownLink(imagePolicy?.sourcePath ?? "/", src);
  }, [imagePolicy, src]);

  useEffect(() => {
    if (imageIntent.kind !== "internal" || !imagePolicy) return;

    const path = imageIntent.path;
    const placeholder = placeholderRef.current;
    if (!placeholder || typeof IntersectionObserver === "undefined") {
      setNearViewportPath(path);
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        setNearViewportPath(path);
        observer.disconnect();
      },
      { root: viewportRoot?.current ?? null, rootMargin: "600px 0px" }
    );
    observer.observe(placeholder);
    return () => observer.disconnect();
  }, [imageIntent, imagePolicy, viewportRoot]);

  useEffect(() => {
    if (imageIntent.kind !== "internal" || !imagePolicy || nearViewportPath !== imageIntent.path) return;

    let active = true;
    const path = imageIntent.path;
    setState({ status: "loading", path });

    const load = retriedPath === path
      ? imagePolicy.loadInternalImage(path, { forceRefresh: true })
      : imagePolicy.loadInternalImage(path);
    void load
      .then((result) => {
        if (!active) return;
        if (result.status === "loaded") {
          setState({ status: "loaded", path, url: result.url });
          return;
        }
        setState({ status: result.status, path });
      })
      .catch(() => {
        if (active) setState({ status: "error", path });
      });

    return () => {
      active = false;
    };
  }, [imageIntent, imagePolicy, nearViewportPath, retriedPath]);

  if (!src) return <ImageFallback alt={alt} message="Image unavailable" />;
  if (imageIntent.kind === "external") return <ExternalMarkdownImage key={src} {...props} src={src} alt={alt} />;
  if (imageIntent.kind === "invalid") return <ImageFallback alt={alt} message="Invalid image link" />;
  if (!imagePolicy) return <ImageFallback alt={alt} message="Image unavailable" />;
  if (state.status === "loaded" && state.path === imageIntent.path) {
    return (
      <img
        {...props}
        src={state.url}
        alt={alt ?? ""}
        loading="lazy"
        decoding="async"
        onError={() => {
          if (retriedPath === imageIntent.path) {
            setState({ status: "error", path: imageIntent.path });
            return;
          }
          setRetriedPath(imageIntent.path);
        }}
      />
    );
  }
  if (state.status === "not-found" && state.path === imageIntent.path) return <ImageFallback alt={alt} message="Image not found" />;
  if (state.status === "unsupported" && state.path === imageIntent.path) return <ImageFallback alt={alt} message="Image cannot be displayed" />;
  if (state.status === "error" && state.path === imageIntent.path) return <ImageFallback alt={alt} message="Could not load image" />;
  return <ImageFallback alt={alt} message="Loading image..." containerRef={placeholderRef} />;
}

function ExternalMarkdownImage({ src, alt, ...props }: ComponentProps<"img">) {
  const [shouldLoad, setShouldLoad] = useState(false);
  const [failed, setFailed] = useState(false);

  if (!shouldLoad) {
    const label = alt ? `Load external image: ${alt}` : "Load external image";
    return <button type="button" className="markdown-image-fallback" onClick={() => setShouldLoad(true)}>{label}</button>;
  }
  if (failed) return <ImageFallback alt={alt} message="Could not load external image" />;
  return (
    <img
      {...props}
      src={src}
      alt={alt ?? ""}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  );
}

function ImageFallback({ alt, message, containerRef }: { alt?: string; message: string; containerRef?: Ref<HTMLSpanElement> }) {
  return <span ref={containerRef} className="markdown-image-fallback">{alt ? `${message}: ${alt}` : message}</span>;
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

function CodeFallback({ content }: { content: string }) {
  const preRef = useRef<HTMLPreElement | null>(null);
  useResetHorizontalScrollOnGrow(preRef);

  return (
    <pre ref={preRef} className="ng-code-fallback" tabIndex={0}>
      <code>{content}</code>
    </pre>
  );
}
