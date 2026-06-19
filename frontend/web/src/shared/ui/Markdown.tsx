import type { Components } from "react-markdown";
import { useRef, type ComponentProps } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { parse as parseYaml } from "yaml";

import { MermaidBlock } from "../../features/editor/MermaidBlock";
import { ShikiCodeBlock } from "../../features/editor/ShikiCodeBlock";
import { useResetHorizontalScrollOnGrow } from "../../features/editor/useResetHorizontalScrollOnGrow";

const components: Components = {
  table({ children, ...props }) {
    return <TableBlock {...props}>{children}</TableBlock>;
  },
  pre({ children }) {
    return <>{children}</>;
  },
  code({ className, children, node, ...props }) {
    const content = String(children).replace(/\n$/, "");
    const language = /language-(\w+)/.exec(className ?? "")?.[1];
    const isBlock = Boolean(node?.position && node.position.start.line !== node.position.end.line);

    if (!language) {
      if (isBlock) {
        return <CodeFallback content={content} />;
      }

      return <code className={className} {...props}>{children}</code>;
    }

    if (language.toLowerCase() === "mermaid") {
      return <MermaidBlock code={content} />;
    }

    return <ShikiCodeBlock code={content} language={language} />;
  }
};

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
export function Markdown({ content }: { content: string }) {
  const document = parseMarkdownDocument(content);

  return (
    <div className="markdown">
      <FrontmatterProperties properties={document.frontmatter} />
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>{document.body}</ReactMarkdown>
    </div>
  );
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
