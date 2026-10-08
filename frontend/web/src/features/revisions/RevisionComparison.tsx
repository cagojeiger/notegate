import { ChevronDown, ChevronUp } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../shared/ui";
import { describeRevisionDiff, type DiffLine, type DiffRow, type RevisionDiff } from "./revisionDiff";

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
  return <ComparisonContent rows={result.rows} />;
}

function ComparisonContent({ rows }: { rows: DiffRow[] }) {
  const { sections, changes, added, removed } = useMemo(() => describeRevisionDiff(rows), [rows]);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());
  const [activeChange, setActiveChange] = useState(0);
  const viewport = useRef<HTMLDivElement>(null);
  const heading = useRef<HTMLDivElement>(null);
  const navigatedScrollTop = useRef<number | null>(null);

  useEffect(() => {
    navigatedScrollTop.current = scrollToChange(viewport.current, heading.current, changes[0]?.start);
  }, [changes]);

  useEffect(() => {
    const container = viewport.current;
    if (!container || !changes.length) return;
    const targets = changes.map(({ start }) => container.querySelector<HTMLElement>(`[data-change-start="${start}"] [data-changed-row]`));
    let frame = 0;
    function update() {
      if (!container) return;
      // Keep the requested section when scrolling is clamped (several changes
      // may fit on screen). Recompute only when the viewport moves again.
      if (container.scrollTop === navigatedScrollTop.current) return;
      navigatedScrollTop.current = null;
      const top = container.getBoundingClientRect().top + (heading.current?.offsetHeight ?? 0) + 2;
      let index = 0;
      targets.forEach((target, candidate) => {
        if (target && target.getBoundingClientRect().top <= top) index = candidate;
      });
      // A short final section cannot always reach the sticky heading.
      if (container.scrollHeight > container.clientHeight + 1 && container.scrollTop >= container.scrollHeight - container.clientHeight - 1) index = changes.length - 1;
      setActiveChange(index);
    }
    function schedule() {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(update);
    }
    container.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      cancelAnimationFrame(frame);
      container.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [changes, expanded]);

  function navigate(index: number) {
    setActiveChange(index);
    navigatedScrollTop.current = scrollToChange(viewport.current, heading.current, changes[index].start);
  }

  if (!changes.length) return <p role="status" className="p-4 text-muted">No changes compared with the current saved version.</p>;
  return (
    <div className="flex min-h-48 flex-1 shrink-0 flex-col md:min-h-0 md:shrink">
      <div className="mb-2 flex shrink-0 flex-wrap items-center justify-between gap-2 text-xs">
        <p aria-label="Change summary" className="shrink-0"><span className="text-success">+{added}<span className="sr-only md:not-sr-only"> added</span></span><span className="ml-2 text-danger">−{removed}<span className="sr-only md:not-sr-only"> removed</span></span></p>
        <div className="flex items-center gap-2">
          <span role="status" aria-label={`Change ${activeChange + 1} of ${changes.length}`}><span className="hidden md:inline">Change </span>{activeChange + 1}<span className="hidden md:inline"> of </span><span className="md:hidden">/</span>{changes.length}</span>
          <Button secondary size="xs" aria-label="Previous change" disabled={activeChange === 0} onClick={() => navigate(activeChange - 1)}><ChevronUp size={16} aria-hidden="true" /></Button>
          <Button secondary size="xs" aria-label="Next change" disabled={activeChange === changes.length - 1} onClick={() => navigate(activeChange + 1)}><ChevronDown size={16} aria-hidden="true" /></Button>
        </div>
      </div>
      <div ref={viewport} role="region" tabIndex={0} className="min-h-36 flex-1 overflow-auto rounded-workbench border border-seam outline-none focus-visible:ring-2 focus-visible:ring-primary/45 md:min-h-0" aria-label="Version comparison">
        <div ref={heading} className="sticky top-0 z-10 border-b border-seam bg-panel">
          <div className="hidden grid-cols-2 text-workbench font-medium md:grid">
            <div className="p-3">Selected version · − removed</div><div className="border-l border-seam p-3">Current saved version · + added</div>
          </div>
          <p className="p-2 text-xs text-muted md:hidden">− Selected version / + Current saved version</p>
        </div>
        {sections.map((section) => {
          const isGap = section.kind === "unchanged";
          const open = expanded.has(section.start);
          return <div key={section.start} data-change-start={isGap ? undefined : section.start} className={!isGap && section === changes[activeChange] ? "ring-1 ring-inset ring-primary/40" : undefined}>
            {isGap ? <Button variant="ghost" size="sm" className="w-full rounded-none border-y border-seam bg-surface text-xs" aria-expanded={open} onClick={() => setExpanded((previous) => {
              const next = new Set(previous);
              if (open) next.delete(section.start); else next.add(section.start);
              return next;
            })}>{section.end - section.start} unchanged lines · {open ? "Collapse" : "Expand"}</Button> : null}
            {!isGap || open ? <div className="font-mono text-xs leading-6">{rows.slice(section.start, section.end).map((row, index) => (
              <div key={section.start + index} data-changed-row={row.kind === "change" ? "" : undefined} className="grid md:grid-cols-2">
                <Line line={row.before} kind={row.kind === "same" ? "same" : "removed"} />
                <Line line={row.after} kind={row.kind === "same" ? "same" : "added"} second />
              </div>
            ))}</div> : null}
          </div>;
        })}
      </div>
    </div>
  );
}

function scrollToChange(container: HTMLDivElement | null, heading: HTMLDivElement | null, start: number | undefined) {
  const target = container?.querySelector<HTMLElement>(`[data-change-start="${start}"] [data-changed-row]`);
  if (!container || !target) return null;
  // Scroll only the comparison, preserving the modal and keyboard focus.
  container.scrollTop += target.getBoundingClientRect().top - container.getBoundingClientRect().top - (heading?.offsetHeight ?? 0) - 1;
  return container.scrollTop;
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
