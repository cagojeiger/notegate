import type { RestNode } from "../api/types";
import { Button, Card, MetaRow, SectionHeader } from "../shared/ui";

export function AuxiliarySidebar({ activeNode, onReplaceMetadata }: { activeNode: RestNode | null; onReplaceMetadata: () => void }) {
  return (
    <aside className="h-full w-full min-h-0 overflow-y-auto border-l border-seam bg-panel p-3">
      <div className="rounded-xl bg-surface px-3 py-1.5 text-sm font-medium">Inspector</div>
      {activeNode ? (
        <div className="mt-4 space-y-3">
          <Card as="section">
            <SectionHeader title="Node" />
            <dl className="space-y-2">
              <MetaRow label="Kind" value={activeNode.kind} />
              <MetaRow label="Name" value={activeNode.name === "/" ? "Space root" : activeNode.name} />
              <MetaRow label="Path" value={activeNode.path} />
              <MetaRow label="Node id" value={activeNode.id} />
              <MetaRow label="Created" value={`${activeNode.created_by.display_name || "—"} · ${activeNode.created_at.slice(0, 10)}`} />
              <MetaRow label="Updated" value={`${activeNode.updated_by.display_name || "—"} · ${activeNode.updated_at.slice(0, 10)}`} />
              {activeNode.byte_len !== undefined ? <MetaRow label="Bytes" value={activeNode.byte_len} /> : null}
              {activeNode.line_count !== undefined ? <MetaRow label="Lines" value={activeNode.line_count} /> : null}
            </dl>
          </Card>
          <Card as="section">
            <SectionHeader title="Metadata" />
            <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{JSON.stringify(activeNode.metadata, null, 2)}</pre>
            <Button size="sm" secondary className="mt-3" onClick={onReplaceMetadata}>Edit metadata</Button>
          </Card>
          <Card as="section">
            <SectionHeader title="Policy" />
            <p className="text-xs leading-5 text-muted">Metadata is not encrypted content. Keep sensitive values inside encrypted text or local client state.</p>
          </Card>
        </div>
      ) : null}
    </aside>
  );
}
