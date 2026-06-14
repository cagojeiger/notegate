import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { MermaidBlock } from "../../features/editor/MermaidBlock";
import { ShikiCodeBlock } from "../../features/editor/ShikiCodeBlock";

const components: Components = {
  pre({ children }) {
    return <>{children}</>;
  },
  code({ className, children, node, ...props }) {
    const content = String(children).replace(/\n$/, "");
    const language = /language-(\w+)/.exec(className ?? "")?.[1];
    const isBlock = Boolean(node?.position && node.position.start.line !== node.position.end.line);

    if (!language) {
      if (isBlock) {
        return (
          <pre className="ng-code-fallback" tabIndex={0}>
            <code>{content}</code>
          </pre>
        );
      }

      return <code className={className} {...props}>{children}</code>;
    }

    if (language.toLowerCase() === "mermaid") {
      return <MermaidBlock code={content} />;
    }

    return <ShikiCodeBlock code={content} language={language} />;
  }
};

// Renders CommonMark + GitHub-flavored markdown. Raw HTML is intentionally not
// enabled (no rehype-raw), so embedded HTML is escaped — safe by default.
export function Markdown({ content }: { content: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>{content}</ReactMarkdown>
    </div>
  );
}
