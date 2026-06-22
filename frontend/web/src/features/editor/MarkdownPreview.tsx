import { Markdown } from "../../shared/ui/Markdown";
import type { MarkdownImagePolicy, MarkdownLinkPolicy } from "../../shared/lib/markdownLinks";

export function MarkdownPreview({ content, linkPolicy, imagePolicy }: { content: string; linkPolicy?: MarkdownLinkPolicy; imagePolicy?: MarkdownImagePolicy }) {
  return <div className="min-h-0 w-full flex-1 overflow-y-auto px-5 py-8 md:px-6 md:py-10 lg:px-8 lg:py-12"><Markdown content={content} linkPolicy={linkPolicy} imagePolicy={imagePolicy} /></div>;
}
