import { lazy, Suspense } from "react";

import { ShikiCodeBlock } from "./ShikiCodeBlock";
import { inferTextFormat, isStructuredFormat, shikiLangForFormat } from "./textFormat";

const MarkdownPreview = lazy(() => import("./MarkdownPreview").then((module) => ({ default: module.MarkdownPreview })));
const StructuredPreview = lazy(() => import("./StructuredPreview").then((module) => ({ default: module.StructuredPreview })));

export function TextPreview({ name, content }: { name: string; content: string }) {
  const format = inferTextFormat(name);

  if (format === "markdown") {
    return <PreviewSuspense><MarkdownPreview content={content} /></PreviewSuspense>;
  }

  if (isStructuredFormat(format)) {
    return <PreviewSuspense><StructuredPreview format={format} content={content} /></PreviewSuspense>;
  }

  return (
    <div className="mx-auto w-full max-w-[52rem] overflow-y-auto px-10 py-14">
      <ShikiCodeBlock code={content} language={shikiLangForFormat(format)} />
    </div>
  );
}

function PreviewSuspense({ children }: { children: React.ReactNode }) {
  return <Suspense fallback={<div className="p-10 text-muted">Preparing preview…</div>}>{children}</Suspense>;
}
