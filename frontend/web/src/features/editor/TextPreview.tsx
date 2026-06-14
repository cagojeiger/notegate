import { lazy, Suspense } from "react";

import { PlainTextPreview } from "./PlainTextPreview";
import { inferTextFormat, isStructuredFormat } from "./textFormat";
import type { StructuredPreviewMode } from "./StructuredPreview";
import type { StructuredExpansionMode } from "./StructuredTreeView";

const MarkdownPreview = lazy(() => import("./MarkdownPreview").then((module) => ({ default: module.MarkdownPreview })));
const StructuredPreview = lazy(() => import("./StructuredPreview").then((module) => ({ default: module.StructuredPreview })));

export function TextPreview({ name, content, structuredMode = "tree", structuredExpansionMode = "expanded" }: { name: string; content: string; structuredMode?: StructuredPreviewMode; structuredExpansionMode?: StructuredExpansionMode }) {
  const format = inferTextFormat(name);

  if (format === "markdown") {
    return <PreviewSuspense><MarkdownPreview content={content} /></PreviewSuspense>;
  }

  if (isStructuredFormat(format)) {
    return <PreviewSuspense><StructuredPreview format={format} content={content} mode={structuredMode} expansionMode={structuredExpansionMode} /></PreviewSuspense>;
  }

  return <PlainTextPreview content={content} />;
}

function PreviewSuspense({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={<div className="p-10 text-muted">Preparing preview…</div>}>{children}</Suspense>;
}
