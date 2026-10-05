import { useEffect, useState } from "react";

import type { DiffLine, RevisionDiff } from "./revisionDiff";

type Result = RevisionDiff | { status: "loading" };

export function RevisionComparison({ before, after }: { before: string; after: string }) {
  const [result, setResult] = useState<Result>({ status: "loading" });
  useEffect(() => {
    let worker: Worker;
    try {
      worker = new Worker(new URL("./revisionDiff.worker.ts", import.meta.url), { type: "module" });
    } catch {
      setResult({ status: "limited" });
      return;
    }
    const timeout = window.setTimeout(() => {
      worker.terminate();
      setResult({ status: "limited" });
    }, 2_000);
    worker.onmessage = (event: MessageEvent<RevisionDiff>) => {
      window.clearTimeout(timeout);
      setResult(event.data);
      worker.terminate();
    };
    worker.onerror = () => {
      window.clearTimeout(timeout);
      worker.terminate();
      setResult({ status: "limited" });
    };
    worker.postMessage({ before, after });
    return () => { window.clearTimeout(timeout); worker.terminate(); };
  }, [before, after]);

  if (result.status === "loading") return <p role="status" className="p-4 text-muted">Comparing versions…</p>;
  if (result.status === "limited") return <p role="status" className="p-4 text-muted">Comparison is unavailable or exceeds the size limit. Use Full version to read the selected content.</p>;
  return (
    <div role="region" tabIndex={0} className="min-h-36 flex-1 shrink-0 overflow-auto rounded-workbench border border-seam outline-none focus-visible:ring-2 focus-visible:ring-primary/45 md:min-h-0 md:shrink" aria-label="Version comparison">
      <div className="sticky top-0 z-10 hidden grid-cols-2 border-b border-seam bg-panel text-workbench font-medium md:grid">
        <div className="p-3">Selected version · − removed</div><div className="border-l border-seam p-3">Current saved version · + added</div>
      </div>
      <p className="sticky top-0 z-10 bg-panel p-2 text-xs text-muted md:hidden">− Selected version / + Current saved version</p>
      <div className="font-mono text-xs leading-6">
        {result.rows.map((row, index) => (
          <div key={index} className="grid md:grid-cols-2">
            <Line line={row.before} kind={row.kind === "same" ? "same" : "removed"} />
            <Line line={row.after} kind={row.kind === "same" ? "same" : "added"} second />
          </div>
        ))}
      </div>
    </div>
  );
}

function Line({ line, kind, second = false }: { line: DiffLine | null; kind: "same" | "removed" | "added"; second?: boolean }) {
  const color = kind === "removed" ? "bg-danger/10 text-text" : kind === "added" ? "bg-success/10 text-text" : "text-text";
  return <div className={`flex min-w-0 items-start ${second ? "md:border-l md:border-seam" : ""} ${!line || (second && kind === "same") ? "hidden md:flex" : ""} ${line ? color : ""}`}>
    {line && kind !== "same" ? <span className="sr-only">{kind === "added" ? "Added: " : "Removed: "}</span> : null}
    <span aria-hidden="true" className="w-10 shrink-0 select-none px-2 text-right text-muted">{line?.number}</span>
    <span aria-hidden="true" className="w-4 shrink-0 select-none">{line && kind !== "same" ? kind === "added" ? "+" : "−" : " "}</span>
    <span className="min-w-0 whitespace-pre-wrap break-all pr-2">{line ? line.text.replace(/\r/g, "␍") || "\u00a0" : "\u00a0"}</span>
  </div>;
}
