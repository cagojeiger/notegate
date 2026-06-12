import type { ReactNode } from "react";

import type { RestNode } from "../api/types";

export function AuxiliarySidebar({ activeNode, onReplaceMetadata }: { activeNode: RestNode | null; onReplaceMetadata: () => void }) {
  return (
    <aside className="min-h-0 border-l border-border bg-panel p-3">
      <div className="grid grid-cols-2 rounded-xl bg-surface p-1 text-sm">
        <button className="rounded-lg bg-panel-strong px-3 py-1.5 font-medium">Inspector</button>
        <button className="rounded-lg px-3 py-1.5 text-muted">Agent</button>
      </div>
      {activeNode ? (
        <div className="mt-4 space-y-3">
          <InspectorCard title="Node">
            <dl className="grid grid-cols-[80px_1fr] gap-y-2 text-sm">
              <dt className="font-semibold text-text">Kind</dt><dd className="text-muted">{activeNode.kind}</dd>
              <dt className="font-semibold text-text">Path</dt><dd className="break-all text-muted">{activeNode.path}</dd>
              <dt className="font-semibold text-text">Updated</dt><dd className="text-muted">{activeNode.updated_at.slice(0, 10)}</dd>
              {activeNode.byte_len !== undefined ? <dt className="font-semibold text-text">Bytes</dt> : null}
              {activeNode.byte_len !== undefined ? <dd className="text-muted">{activeNode.byte_len}</dd> : null}
            </dl>
          </InspectorCard>
          <InspectorCard title="Metadata">
            <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{JSON.stringify(activeNode.metadata, null, 2)}</pre>
            <button className="mt-3 rounded-lg border border-border bg-surface px-3 py-1 text-xs text-muted hover:bg-panel hover:text-text" onClick={onReplaceMetadata}>Edit metadata</button>
          </InspectorCard>
        </div>
      ) : null}
    </aside>
  );
}

function InspectorCard({ title, children }: { title: string; children: ReactNode }) {
  return <section className="rounded-2xl border border-border bg-surface p-4"><h3 className="mb-3 text-xs font-bold uppercase tracking-wide text-muted">{title}</h3>{children}</section>;
}
