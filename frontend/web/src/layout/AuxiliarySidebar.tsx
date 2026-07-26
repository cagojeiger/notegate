import { LockKeyhole, Search } from "lucide-react";

import type { RestNode } from "../api/types";
import { Button, MetaRow, SectionHeader, SettingToggle } from "../shared/ui";

const EMPTY = "—";

type AuxiliarySidebarProps = {
  activeNode: RestNode | null;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  textEncryptionAvailable: boolean;
  searchPolicyPending: boolean;
  textEncryptionPending: boolean;
  onReplaceMetadata: () => void;
  onSearchEnabledChange: (enabled: boolean) => void;
  onTextEncryptionEnabledChange: (enabled: boolean) => void;
};

export function AuxiliarySidebar({
  activeNode,
  canWriteActiveSpace,
  canManageActiveSpace,
  textEncryptionAvailable,
  searchPolicyPending,
  textEncryptionPending,
  onReplaceMetadata,
  onSearchEnabledChange,
  onTextEncryptionEnabledChange
}: AuxiliarySidebarProps) {
  const metadata = activeNode?.metadata ?? {};
  const clientEncrypted = activeNode?.text_storage_format === "encrypted";
  const serverEncrypted = activeNode?.text_at_rest_encryption === "server";

  return (
    <aside className="flex h-full w-full min-h-0 flex-col border-l border-seam bg-panel">
      <div className="flex h-12 shrink-0 items-center border-b border-seam px-3 text-sm font-medium">Inspector</div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
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
            <SectionHeader
              title="Node settings"
              help="Changes apply immediately to this node. Search controls whether it appears in find and grep results. Stored text encryption applies only to text content. The settings are independent."
            />
            {activeNode ? (
              <div className="space-y-3">
                <SettingToggle
                  icon={<Search size={16} />}
                  label="Include in search"
                  checked={activeNode.search_enabled}
                  disabled={
                    !canManageActiveSpace
                    || activeNode.parent_id === null
                    || searchPolicyPending
                  }
                  onChange={onSearchEnabledChange}
                />
                {activeNode.kind === "text" ? (
                  <SettingToggle
                    icon={<LockKeyhole size={16} />}
                    label="Stored text encryption"
                    badge={clientEncrypted ? "Client" : !textEncryptionAvailable ? "Max" : undefined}
                    checked={clientEncrypted || serverEncrypted}
                    disabled={
                      !canManageActiveSpace
                      || clientEncrypted
                      || textEncryptionPending
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
        </div>
      </div>
    </aside>
  );
}
