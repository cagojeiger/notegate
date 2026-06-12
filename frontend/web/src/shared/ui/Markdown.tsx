import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// Renders CommonMark + GitHub-flavored markdown. Raw HTML is intentionally not
// enabled (no rehype-raw), so embedded HTML is escaped — safe by default.
export function Markdown({ content }: { content: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  );
}
