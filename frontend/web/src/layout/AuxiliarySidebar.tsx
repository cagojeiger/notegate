import type { ReactNode } from "react";

import type { RestNode } from "../api/types";

export function AuxiliarySidebar({ activeNode, onReplaceMetadata }: { activeNode: RestNode | null; onReplaceMetadata: () => void }) {
  return (
    <aside className="h-full w-full min-h-0 overflow-y-auto border-l border-seam bg-panel p-3">
      <div className="rounded-xl bg-surface px-3 py-1.5 text-sm font-medium">Inspector</div>
      {activeNode ? (
        <div className="mt-4 space-y-3">
          <InspectorCard title="Node">
            <dl className="grid grid-cols-[72px_1fr] gap-y-2 text-sm">
              <Field label="Kind">{activeNode.kind}</Field>
              <Field label="Name">{activeNode.name === "/" ? "Space root" : activeNode.name}</Field>
              <Field label="Path" breakAll>{activeNode.path}</Field>
              <Field label="Node id" breakAll>{activeNode.id}</Field>
              <Field label="Created">{`${activeNode.created_by.display_name || "—"} · ${activeNode.created_at.slice(0, 10)}`}</Field>
              <Field label="Updated">{`${activeNode.updated_by.display_name || "—"} · ${activeNode.updated_at.slice(0, 10)}`}</Field>
              {activeNode.byte_len !== undefined ? <Field label="Bytes">{activeNode.byte_len}</Field> : null}
              {activeNode.line_count !== undefined ? <Field label="Lines">{activeNode.line_count}</Field> : null}
            </dl>
          </InspectorCard>
          <InspectorCard title="Metadata">
            <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{JSON.stringify(activeNode.metadata, null, 2)}</pre>
            <button className="mt-3 rounded-lg border border-border bg-surface px-3 py-1 text-xs text-muted hover:bg-panel hover:text-text" onClick={onReplaceMetadata}>Edit metadata</button>
          </InspectorCard>
          <InspectorCard title="Policy">
            <p className="text-xs leading-5 text-muted">Metadata is not encrypted content. Keep sensitive values inside encrypted text or local client state.</p>
          </InspectorCard>
        </div>
      ) : null}
    </aside>
  );
}

function InspectorCard({ title, children }: { title: string; children: ReactNode }) {
  return <section className="rounded-2xl border border-border bg-surface p-4"><h3 className="mb-3 text-xs font-bold uppercase tracking-wide text-muted">{title}</h3>{children}</section>;
}

function Field({ label, breakAll, children }: { label: string; breakAll?: boolean; children: ReactNode }) {
  return (
    <>
      <dt className="font-semibold text-text">{label}</dt>
      <dd className={`text-muted ${breakAll ? "break-all" : ""}`}>{children}</dd>
    </>
  );
}
