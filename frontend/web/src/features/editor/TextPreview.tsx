import { lazy, Suspense } from "react";

import { CodePreview } from "./CodePreview";
import { PlainTextPreview } from "./PlainTextPreview";
import { hasMarkdownFrontmatterCandidate } from "../../shared/lib/markdownFrontmatter";
import type { MarkdownImagePolicy, MarkdownLinkPolicy } from "../../shared/lib/markdownLinks";
import type { DelimitedPreviewMode } from "./DelimitedPreview";
import type { MarkdownOutlineIdentity } from "./MarkdownOutlineContext";
import { inferTextFormat, isCodeFormat, isTabularFormat } from "./textFormat";
import type { StructuredPreviewMode } from "./StructuredPreview";
import type { StructuredExpansionMode } from "./StructuredTreeView";

const MarkdownPreview = lazy(() => import("./MarkdownPreview").then((module) => ({ default: module.MarkdownPreview })));
const MarkdownFrontmatterPreview = lazy(() => import("./MarkdownFrontmatterPreview").then((module) => ({ default: module.MarkdownFrontmatterPreview })));
const StructuredPreview = lazy(() => import("./StructuredPreview").then((module) => ({ default: module.StructuredPreview })));
const YamlStructuredPreview = lazy(() => import("./YamlStructuredPreview").then((module) => ({ default: module.YamlStructuredPreview })));
const TomlStructuredPreview = lazy(() => import("./TomlStructuredPreview").then((module) => ({ default: module.TomlStructuredPreview })));
const DelimitedPreview = lazy(() => import("./DelimitedPreview").then((module) => ({ default: module.DelimitedPreview })));

export function TextPreview({ name, content, previewIdentity, markdownLinkPolicy, markdownImagePolicy, markdownOutlineIdentity, structuredMode = "tree", structuredExpansionMode = "expanded", tabularMode = "table" }: { name: string; content: string; previewIdentity?: string; markdownLinkPolicy?: MarkdownLinkPolicy; markdownImagePolicy?: MarkdownImagePolicy; markdownOutlineIdentity?: MarkdownOutlineIdentity; structuredMode?: StructuredPreviewMode; structuredExpansionMode?: StructuredExpansionMode; tabularMode?: DelimitedPreviewMode }) {
  const format = inferTextFormat(name);

  if (format === "markdown") {
    const Preview = hasMarkdownFrontmatterCandidate(content) ? MarkdownFrontmatterPreview : MarkdownPreview;
    return <PreviewSuspense><Preview content={content} linkPolicy={markdownLinkPolicy} imagePolicy={markdownImagePolicy} outlineIdentity={markdownOutlineIdentity} /></PreviewSuspense>;
  }

  if (format === "json" || format === "jsonl") {
    return <PreviewSuspense><StructuredPreview format={format} content={content} mode={structuredMode} expansionMode={structuredExpansionMode} /></PreviewSuspense>;
  }

  if (format === "yaml") {
    return <PreviewSuspense><YamlStructuredPreview content={content} mode={structuredMode} expansionMode={structuredExpansionMode} /></PreviewSuspense>;
  }

  if (format === "toml") {
    return <PreviewSuspense><TomlStructuredPreview content={content} mode={structuredMode} expansionMode={structuredExpansionMode} /></PreviewSuspense>;
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
