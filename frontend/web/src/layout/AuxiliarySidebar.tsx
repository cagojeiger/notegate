import { ChevronRight, LockKeyhole, Search } from "lucide-react";
import { useId, useState } from "react";

import type { RestNode } from "../api/types";
import { useMarkdownOutlineContext, type MarkdownInspectorView, type MarkdownOutlineSnapshot } from "../features/editor/MarkdownOutlineContext";
import { useFolderChildrenStat } from "../features/editor/useEditorQueries";
import { formatBytes } from "../shared/lib/formatBytes";
import { MetaRow, SectionHeader, SettingToggle, Tabs } from "../shared/ui";
import { WriteLockStatus } from "./WriteLockStatus";

const EMPTY = "—";

type AuxiliarySidebarProps = {
  activeNode: RestNode | null;
  activeGroupId?: number | null;
  loadingNode?: boolean;
  canManageActiveSpace: boolean;
  textEncryptionAvailable: boolean;
  writeLockAvailable: boolean;
  searchPolicyPending: boolean;
  writeLockPending: boolean;
  textEncryptionPending: boolean;
  onSearchEnabledChange: (enabled: boolean) => void;
  onWriteLockedChange: (enabled: boolean) => void;
  onTextEncryptionEnabledChange: (enabled: boolean) => void;
  onOutlineNavigate?: () => void;
};

export function AuxiliarySidebar({
  activeNode,
  activeGroupId = null,
  loadingNode = false,
  canManageActiveSpace,
  textEncryptionAvailable,
  writeLockAvailable,
  searchPolicyPending,
  writeLockPending,
  textEncryptionPending,
  onSearchEnabledChange,
  onWriteLockedChange,
  onTextEncryptionEnabledChange,
  onOutlineNavigate
}: AuxiliarySidebarProps) {
  const [localPreferredView, setLocalPreferredView] = useState<MarkdownInspectorView>("details");
  const panelIdPrefix = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const outlineContext = useMarkdownOutlineContext();
  const preferredView = outlineContext?.preferredInspectorView ?? localPreferredView;
  const setPreferredView = outlineContext?.setPreferredInspectorView ?? setLocalPreferredView;
  const outline = activeGroupId === null ? undefined : outlineContext?.outlinesByGroup[activeGroupId];
  const outlineAvailable = Boolean(
    outline
    && activeNode
    && outline.spaceId === activeNode.space_id
    && outline.nodeId === activeNode.id
  );
  const selectedView: MarkdownInspectorView = preferredView === "outline" && outlineAvailable ? "outline" : "details";
  const metadata = activeNode?.metadata ?? {};
  const clientEncrypted = activeNode?.text_storage_format === "encrypted";
  const serverEncrypted = activeNode?.text_at_rest_encryption === "server";
  const changesLocked = activeNode?.effective_write_locked ?? false;
  const metadataEntries = Object.entries(metadata);

  return (
    <aside aria-label="Inspector" className="flex h-full w-full min-h-0 flex-col border-l border-seam bg-panel">
      <div className="flex h-12 shrink-0 items-end border-b border-seam px-3">
        <Tabs
          items={[
            { id: "details", label: "Details", controls: `${panelIdPrefix}-details` },
            { id: "outline", label: "Outline", controls: `${panelIdPrefix}-outline`, disabled: !outlineAvailable }
          ]}
          value={selectedView}
          onChange={setPreferredView}
          label="Inspector sections"
          variant="header"
        />
      </div>
      <div className="min-h-0 flex-1">
      <div
        id={`${panelIdPrefix}-details`}
        role="tabpanel"
        aria-labelledby={`${panelIdPrefix}-details-tab`}
        tabIndex={0}
        hidden={selectedView !== "details"}
        className="h-full overflow-y-auto p-3"
        data-testid="node-inspector-scroll-region"
      >
        <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
          <section className="p-4">
            <SectionHeader title={activeNode ? nodeKindLabel(activeNode) : "Details"} />
            {activeNode ? (
              <>
                <dl className="space-y-2">
                  <MetaRow label="Name" value={activeNode.name === "/" ? "Space root" : activeNode.name} />
                  <MetaRow label="Path" value={activeNode.path} />
                  <MetaRow label="Kind" value={nodeKindLabel(activeNode)} />
                  {activeNode.kind === "folder" ? <FolderChildCount node={activeNode} /> : null}
                  {activeNode.kind !== "folder" && activeNode.byte_len !== undefined ? (
                    <MetaRow label="Size" value={formatBytes(activeNode.byte_len)} />
                  ) : null}
                  {activeNode.kind === "text" && activeNode.line_count !== undefined ? (
                    <MetaRow label="Lines" value={String(activeNode.line_count)} />
                  ) : null}
                </dl>
                <WriteLockStatus
                  key={activeNode.id}
                  nodeId={activeNode.id}
                  directlyLocked={activeNode.write_locked}
                  sources={activeNode.write_lock_sources}
                />
              </>
            ) : (
              <p className="text-xs text-muted">{loadingNode ? "Loading details…" : "Choose something from Files to inspect."}</p>
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
          </section>
          <section className="p-4">
            <SectionHeader
              title="Settings"
              help="Changes apply immediately. A direct lock protects this item and anything inside it; inherited locks must be removed at their source. Search and stored text encryption are independent settings. The space root cannot be locked."
            />
            {activeNode ? (
              <div className="space-y-3">
                <SettingToggle
                  icon={<LockKeyhole size={16} />}
                  label="Lock changes"
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
              <p className="text-xs text-muted">Choose something from Files to manage its settings.</p>
            )}
          </section>
          <details className="group p-4">
            <summary className="flex cursor-pointer list-none items-center justify-between outline-none focus-visible:ring-2 focus-visible:ring-primary/45 [&::-webkit-details-marker]:hidden">
              <span className="text-xs font-semibold uppercase tracking-wide text-muted">System details</span>
              <ChevronRight
                size={16}
                className="text-muted transition-transform group-open:rotate-90"
                aria-hidden="true"
              />
            </summary>
            <dl className="mt-3 space-y-2">
              <MetaRow label="Created" value={activeNode ? `${activeNode.created_by.display_name || EMPTY} · ${activeNode.created_at.slice(0, 10)}` : EMPTY} />
              <MetaRow label="Updated" value={activeNode ? `${activeNode.updated_by.display_name || EMPTY} · ${activeNode.updated_at.slice(0, 10)}` : EMPTY} />
              <MetaRow label="Internal ID" value={activeNode?.id ?? EMPTY} />
            </dl>
          </details>
        </div>
      </div>
      <div
        id={`${panelIdPrefix}-outline`}
        role="tabpanel"
        aria-labelledby={`${panelIdPrefix}-outline-tab`}
        tabIndex={outlineAvailable && outline?.items.length ? undefined : 0}
        hidden={selectedView !== "outline"}
        className="h-full min-h-0 overflow-hidden p-3"
      >
        {outlineAvailable && outline ? (
          <OutlinePanel outline={outline} onNavigate={onOutlineNavigate} />
        ) : null}
      </div>
      </div>
    </aside>
  );
}

function OutlinePanel({ outline, onNavigate }: { outline: MarkdownOutlineSnapshot; onNavigate?: () => void }) {
  if (outline.items.length === 0) {
    return <p className="text-xs text-muted">No headings in this document.</p>;
  }
  const baseLevel = Math.min(...outline.items.map((item) => item.level));

  return (
    <nav
      aria-label="Document outline"
      tabIndex={0}
      className="max-h-full overflow-y-auto rounded-xl border border-seam bg-surface p-1.5 font-ui outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
    >
      <ul className="relative space-y-0.5 before:pointer-events-none before:absolute before:inset-y-1 before:left-1 before:w-px before:bg-seam">
        {outline.items.map((item) => {
          const active = item.id === outline.activeItemId;
          return (
            <li key={item.id} className="relative">
              {active ? (
                <span
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-y-1 left-[3px] z-10 w-0.5 rounded-full bg-[var(--ng-active-border)]"
                />
              ) : null}
              <button
                type="button"
                aria-current={active ? "location" : undefined}
                className={`flex min-h-8 w-full items-start rounded-lg py-1 pr-1.5 text-left text-sm leading-5 transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/45 ${active ? "bg-[var(--ng-active-surface)] font-semibold text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}
                style={{ paddingInlineStart: `${12 + (item.level - baseLevel) * 10}px` }}
                onClick={() => {
                  outline.navigate(item.id);
                  onNavigate?.();
                }}
                title={item.label}
              >
                <span className="line-clamp-2 break-words">{item.label}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

function FolderChildCount({ node }: { node: RestNode }) {
  const childrenQuery = useFolderChildrenStat(node);
  const value = childrenQuery.data
    ? `${childrenQuery.data.children.length}${childrenQuery.data.page.has_more ? "+" : ""}`
    : childrenQuery.isError
      ? EMPTY
      : "…";
  return <MetaRow label="Children" value={value} />;
}

function nodeKindLabel(node: RestNode): string {
  if (node.parent_id === null) return "Space";
  return node.kind === "folder" ? "Folder" : node.kind === "text" ? "Document" : "File";
}
