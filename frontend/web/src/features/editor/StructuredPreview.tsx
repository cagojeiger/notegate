import { useMemo } from "react";

import { parseStructuredText, type StructuredFormat } from "./structuredData";
import { StructuredTreeView, type StructuredExpansionMode } from "./StructuredTreeView";
import { ShikiCodeBlock } from "./ShikiCodeBlock";
import { shikiLangForFormat } from "./textFormat";

export type StructuredPreviewMode = "tree" | "source";

export function StructuredPreview({ format, content, mode = "tree", expansionMode = "expanded" }: { format: StructuredFormat; content: string; mode?: StructuredPreviewMode; expansionMode?: StructuredExpansionMode }) {
  const parsed = useMemo(() => parseStructuredText(format, content), [format, content]);

  return (
    <div className="mx-auto flex min-h-0 w-full max-w-[52rem] flex-1 flex-col overflow-hidden px-10 py-10">
      {mode === "tree" && parsed.ok ? (
        <div className="min-h-0 flex-1 overflow-auto py-2">
          <StructuredTreeView value={parsed.value} expansionMode={expansionMode} />
        </div>
      ) : mode === "tree" && !parsed.ok ? (
        <div className="rounded-2xl border border-danger/30 bg-danger/10 p-4 text-sm text-danger">
          Could not parse {format.toUpperCase()}: {parsed.message}
        </div>
      ) : (
        <div className="ng-source-flat min-h-0 flex-1 overflow-auto py-2">
          <ShikiCodeBlock code={content} language={shikiLangForFormat(format)} />
        </div>
      )}
    </div>
  );
}
