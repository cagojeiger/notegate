import { Markdown } from "../../shared/ui/Markdown";

export function MarkdownPreview({ content }: { content: string }) {
  return <div className="min-h-0 w-full flex-1 overflow-y-auto px-6 py-10 md:px-8 lg:px-10 lg:py-14"><Markdown content={content} /></div>;
}
