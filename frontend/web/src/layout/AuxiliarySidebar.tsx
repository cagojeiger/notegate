import type { RestNode } from "../entities/node/model";
import { Button, MetaRow, SectionHeader } from "../shared/ui";

const EMPTY = "—";

export function AuxiliarySidebar({ activeNode, canWriteActiveSpace, onReplaceMetadata }: { activeNode: RestNode | null; canWriteActiveSpace: boolean; onReplaceMetadata: () => void }) {
  const metadata = activeNode?.metadata ?? {};

  return (
    <aside className="h-full w-full min-h-0 overflow-y-auto border-l border-seam bg-panel p-3">
      <div className="rounded-xl bg-[var(--ng-hover)] px-3 py-1.5 text-sm font-medium">Inspector</div>
      <div className="mt-4 divide-y divide-seam rounded-2xl border border-border bg-surface">
        <section className="p-4">
          <SectionHeader title="Node" />
          <dl className="space-y-2">
            <MetaRow label="Kind" value={activeNode?.kind ?? EMPTY} />
            <MetaRow label="Name" value={activeNode ? (activeNode.name === "/" ? "Space root" : activeNode.name) : EMPTY} />
            <MetaRow label="Path" value={activeNode?.path ?? EMPTY} />
            <MetaRow label="Node id" value={activeNode?.id ?? EMPTY} />
            <MetaRow label="Created" value={activeNode ? `${activeNode.created_by.display_name || EMPTY} · ${activeNode.created_at.slice(0, 10)}` : EMPTY} />
            <MetaRow label="Updated" value={activeNode ? `${activeNode.updated_by.display_name || EMPTY} · ${activeNode.updated_at.slice(0, 10)}` : EMPTY} />
            <MetaRow label="Bytes" value={activeNode?.byte_len ?? EMPTY} />
            <MetaRow label="Lines" value={activeNode?.line_count ?? EMPTY} />
          </dl>
        </section>
        <section className="p-4">
          <SectionHeader title="Metadata" />
          <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{JSON.stringify(metadata, null, 2)}</pre>
          <Button size="sm" secondary className="mt-3" onClick={onReplaceMetadata} disabled={!activeNode || !canWriteActiveSpace}>Edit metadata</Button>
        </section>
        <section className="p-4">
          <SectionHeader title="Policy" />
          <p className="text-xs leading-5 text-muted">Metadata is not encrypted content. Keep sensitive values inside encrypted text or local client state.</p>
        </section>
      </div>
    </aside>
  );
}
