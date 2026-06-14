import { useMemo, useState } from "react";

import { Button } from "../../shared/ui";
import { parseStructuredText, type StructuredFormat } from "./structuredData";
import { StructuredTreeView } from "./StructuredTreeView";
import { ShikiCodeBlock } from "./ShikiCodeBlock";
import { shikiLangForFormat } from "./textFormat";

type PreviewMode = "tree" | "source";

export function StructuredPreview({ format, content }: { format: StructuredFormat; content: string }) {
  const [mode, setMode] = useState<PreviewMode>("tree");
  const parsed = useMemo(() => parseStructuredText(format, content), [format, content]);

  return (
    <div className="mx-auto flex min-h-0 w-full max-w-[52rem] flex-1 flex-col overflow-hidden px-10 py-10">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-xs font-semibold uppercase tracking-[0.12em] text-muted">Structured preview</div>
          <div className="mt-1 text-sm text-muted">{parsed.ok ? parsed.label : format.toUpperCase()}</div>
        </div>
        <div className="flex gap-2">
          <Button size="xs" variant={mode === "tree" ? "primary" : "secondary"} onClick={() => setMode("tree")} disabled={!parsed.ok}>Tree</Button>
          <Button size="xs" variant={mode === "source" ? "primary" : "secondary"} onClick={() => setMode("source")}>Source</Button>
        </div>
      </div>

      {mode === "tree" && parsed.ok ? (
        <div className="min-h-0 flex-1 overflow-auto rounded-2xl border border-border bg-surface p-4">
          <StructuredTreeView value={parsed.value} />
        </div>
      ) : mode === "tree" && !parsed.ok ? (
        <div className="rounded-2xl border border-danger/30 bg-danger/10 p-4 text-sm text-danger">
          Could not parse {format.toUpperCase()}: {parsed.message}
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto rounded-2xl border border-border bg-surface">
          <ShikiCodeBlock code={content} language={shikiLangForFormat(format)} />
        </div>
      )}
    </div>
  );
}
