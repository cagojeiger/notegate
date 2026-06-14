import { Markdown } from "../../shared/ui/Markdown";

export function MarkdownPreview({ content }: { content: string }) {
  return <div className="mx-auto w-full max-w-[44rem] overflow-y-auto px-10 py-14"><Markdown content={content} /></div>;
}
