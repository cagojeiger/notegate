import { ChevronRight, LockKeyhole, Search } from "lucide-react";

import type { RestNode } from "../api/types";
import { formatBytes } from "../shared/lib/formatBytes";
import { Button, MetaRow, SectionHeader, SettingToggle } from "../shared/ui";
import { WriteLockStatus } from "./WriteLockStatus";

const EMPTY = "—";

type AuxiliarySidebarProps = {
  activeNode: RestNode | null;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  textEncryptionAvailable: boolean;
  writeLockAvailable: boolean;
  searchPolicyPending: boolean;
  writeLockPending: boolean;
  textEncryptionPending: boolean;
  onReplaceMetadata: () => void;
  onSearchEnabledChange: (enabled: boolean) => void;
  onWriteLockedChange: (enabled: boolean) => void;
  onTextEncryptionEnabledChange: (enabled: boolean) => void;
};

export function AuxiliarySidebar({
  activeNode,
  canWriteActiveSpace,
  canManageActiveSpace,
  textEncryptionAvailable,
  writeLockAvailable,
  searchPolicyPending,
  writeLockPending,
  textEncryptionPending,
  onReplaceMetadata,
  onSearchEnabledChange,
  onWriteLockedChange,
  onTextEncryptionEnabledChange
}: AuxiliarySidebarProps) {
  const metadata = activeNode?.metadata ?? {};
  const clientEncrypted = activeNode?.text_storage_format === "encrypted";
  const serverEncrypted = activeNode?.text_at_rest_encryption === "server";
  const changesLocked = activeNode?.effective_write_locked ?? false;
  const metadataEntries = Object.entries(metadata);

  return (
    <aside className="flex h-full w-full min-h-0 flex-col border-l border-seam bg-panel">
      <div className="flex h-12 shrink-0 items-center border-b border-seam px-3 text-sm font-medium">Inspector</div>
      <div
        className="min-h-0 flex-1 overflow-y-auto p-3"
        data-testid="node-inspector-scroll-region"
      >
        <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
          <section className="p-4">
            <SectionHeader title="Node" />
            {activeNode ? (
              <>
                <dl className="space-y-2">
                  <MetaRow label="Name" value={activeNode.name === "/" ? "Space root" : activeNode.name} />
                  <MetaRow label="Path" value={activeNode.path} />
                  <MetaRow label="Kind" value={nodeSummary(activeNode)} />
                </dl>
                <WriteLockStatus
                  key={activeNode.id}
                  nodeId={activeNode.id}
                  directlyLocked={activeNode.write_locked}
                  sources={activeNode.write_lock_sources}
                />
              </>
            ) : (
              <p className="text-xs text-muted">Select a node to inspect.</p>
            )}
          </section>
          <section className="p-4">
            <SectionHeader
              title="Metadata"
              actions={<span className="text-xs tabular-nums text-muted">{metadataEntries.length}</span>}
            />
            {metadataEntries.length > 0 ? (
              <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{JSON.stringify(metadata, null, 2)}</pre>
            ) : (
              <p className="text-xs text-muted">No metadata.</p>
            )}
            <Button
              size="sm"
              secondary
              className="mt-3"
              onClick={onReplaceMetadata}
              disabled={!activeNode || !canWriteActiveSpace || changesLocked}
            >
              Edit metadata
            </Button>
          </section>
          <section className="p-4">
            <SectionHeader
              title="Node settings"
              help="Changes apply immediately to this node. A direct lock protects this node and its descendants; inherited locks must be removed at their source. Search and stored text encryption are independent settings. The space root cannot be locked."
            />
            {activeNode ? (
              <div className="space-y-3">
                <SettingToggle
                  icon={<LockKeyhole size={16} />}
                  label="Lock this node"
                  badge={
                    activeNode.parent_id === null
                      ? "Root"
                      : !writeLockAvailable && !activeNode.write_locked
                        ? "Unavailable"
                        : undefined
                  }
                  checked={activeNode.write_locked}
                  disabled={
                    !canManageActiveSpace
                    || activeNode.parent_id === null
                    || writeLockPending
                    || (!writeLockAvailable && !activeNode.write_locked)
                  }
                  onChange={onWriteLockedChange}
                />
                <SettingToggle
                  icon={<Search size={16} />}
                  label="Include in search"
                  checked={activeNode.search_enabled}
                  disabled={
                    !canManageActiveSpace
                    || activeNode.parent_id === null
                    || searchPolicyPending
                    || changesLocked
                  }
                  onChange={onSearchEnabledChange}
                />
                {activeNode.kind === "text" ? (
                  <SettingToggle
                    icon={<LockKeyhole size={16} />}
                    label="Stored text encryption"
                    badge={clientEncrypted ? "Client" : !textEncryptionAvailable && !serverEncrypted ? "Unavailable" : undefined}
                    checked={clientEncrypted || serverEncrypted}
                    disabled={
                      !canManageActiveSpace
                      || clientEncrypted
                      || textEncryptionPending
                      || changesLocked
                      || (!textEncryptionAvailable && !serverEncrypted)
                    }
                    onChange={onTextEncryptionEnabledChange}
                  />
                ) : null}
              </div>
            ) : (
              <p className="text-xs text-muted">Select a node to manage its settings.</p>
            )}
          </section>
          <details className="group p-4">
            <summary className="flex cursor-pointer list-none items-center justify-between outline-none focus-visible:ring-2 focus-visible:ring-primary/45 [&::-webkit-details-marker]:hidden">
              <span className="text-xs font-semibold uppercase tracking-wide text-muted">Details</span>
              <ChevronRight
                size={16}
                className="text-muted transition-transform group-open:rotate-90"
                aria-hidden="true"
              />
            </summary>
            <dl className="mt-3 space-y-2">
              <MetaRow label="Created" value={activeNode ? `${activeNode.created_by.display_name || EMPTY} · ${activeNode.created_at.slice(0, 10)}` : EMPTY} />
              <MetaRow label="Updated" value={activeNode ? `${activeNode.updated_by.display_name || EMPTY} · ${activeNode.updated_at.slice(0, 10)}` : EMPTY} />
              <MetaRow label="Node id" value={activeNode?.id ?? EMPTY} />
            </dl>
          </details>
        </div>
      </div>
    </aside>
  );
}

function nodeSummary(node: RestNode): string {
  const kindLabel = node.kind === "folder" ? "Folder" : node.kind === "text" ? "Text" : "File";
  const parts = [kindLabel];
  if (node.kind !== "folder" && node.byte_len !== undefined) {
    parts.push(formatBytes(node.byte_len));
  }
  if (node.kind === "text" && node.line_count !== undefined) {
    parts.push(`${node.line_count} ${node.line_count === 1 ? "line" : "lines"}`);
  }
  return parts.join(" · ");
}
