import { lazy, Suspense } from "react";

import { CodePreview } from "./CodePreview";
import { PlainTextPreview } from "./PlainTextPreview";
import type { MarkdownImagePolicy, MarkdownLinkPolicy } from "../../shared/lib/markdownLinks";
import type { DelimitedPreviewMode } from "./DelimitedPreview";
import type { MarkdownOutlineIdentity } from "./MarkdownOutlineContext";
import { inferTextFormat, isCodeFormat, isStructuredFormat, isTabularFormat } from "./textFormat";
import type { StructuredPreviewMode } from "./StructuredPreview";
import type { StructuredExpansionMode } from "./StructuredTreeView";

const MarkdownPreview = lazy(() => import("./MarkdownPreview").then((module) => ({ default: module.MarkdownPreview })));
const StructuredPreview = lazy(() => import("./StructuredPreview").then((module) => ({ default: module.StructuredPreview })));
const DelimitedPreview = lazy(() => import("./DelimitedPreview").then((module) => ({ default: module.DelimitedPreview })));

export function TextPreview({ name, content, previewIdentity, markdownLinkPolicy, markdownImagePolicy, markdownOutlineIdentity, structuredMode = "tree", structuredExpansionMode = "expanded", tabularMode = "table" }: { name: string; content: string; previewIdentity?: string; markdownLinkPolicy?: MarkdownLinkPolicy; markdownImagePolicy?: MarkdownImagePolicy; markdownOutlineIdentity?: MarkdownOutlineIdentity; structuredMode?: StructuredPreviewMode; structuredExpansionMode?: StructuredExpansionMode; tabularMode?: DelimitedPreviewMode }) {
  const format = inferTextFormat(name);

  if (format === "markdown") {
    return <PreviewSuspense><MarkdownPreview content={content} linkPolicy={markdownLinkPolicy} imagePolicy={markdownImagePolicy} outlineIdentity={markdownOutlineIdentity} /></PreviewSuspense>;
  }

  if (isStructuredFormat(format)) {
    return <PreviewSuspense><StructuredPreview format={format} content={content} mode={structuredMode} expansionMode={structuredExpansionMode} /></PreviewSuspense>;
  }

  if (isTabularFormat(format)) {
    return <PreviewSuspense><DelimitedPreview format={format} content={content} mode={tabularMode} identity={previewIdentity} /></PreviewSuspense>;
  }

  if (isCodeFormat(format)) {
    return <CodePreview format={format} content={content} />;
  }

  return <PlainTextPreview content={content} />;
}

function PreviewSuspense({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={<div className="p-10 text-muted">Preparing preview…</div>}>{children}</Suspense>;
}
