import { useMemo } from "react";

import { parseMarkdownDocument } from "../../shared/lib/markdownDocument";
import { MarkdownPreview, type MarkdownPreviewProps } from "./MarkdownPreview";

export function MarkdownFrontmatterPreview({ content, ...props }: Omit<MarkdownPreviewProps, "frontmatter">) {
  const document = useMemo(() => parseMarkdownDocument(content), [content]);

  return <MarkdownPreview {...props} content={document.body} frontmatter={document.frontmatter} />;
}
