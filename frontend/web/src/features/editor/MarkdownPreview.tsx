import { Markdown } from "../../shared/ui/Markdown";

export function MarkdownPreview({ content }: { content: string }) {
  return <div className="min-h-0 w-full flex-1 overflow-y-auto px-5 py-8 md:px-6 md:py-10 lg:px-8 lg:py-12"><Markdown content={content} /></div>;
}
