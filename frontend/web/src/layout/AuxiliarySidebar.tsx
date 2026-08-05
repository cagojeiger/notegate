import { ChevronRight, Image as ImageIcon, Link2, LockKeyhole, RefreshCw, Search, Unlink } from "lucide-react";
import { useId, useState } from "react";

import type { LinkReference, NodeLinkIndexResponse, RestNode } from "../api/types";
import { useMarkdownOutlineContext, type MarkdownInspectorView, type MarkdownOutlineSnapshot } from "../features/editor/MarkdownOutlineContext";
import { useFolderChildrenStat } from "../features/editor/useEditorQueries";
import { formatLinkIndexSyncTime, linkIndexStatusLabel } from "../features/links/linkIndexPresentation";
import { useNodeLinkIndexQuery, useSyncNodeLinkIndexMutation } from "../features/links/useLinkIndexQueries";
import { formatBytes } from "../shared/lib/formatBytes";
import { Button, MetaRow, SectionHeader, SettingToggle, Tabs } from "../shared/ui";
import { WriteLockStatus } from "./WriteLockStatus";

const EMPTY = "—";

type AuxiliarySidebarProps = {
  activeNode: RestNode | null;
  activeGroupId?: number | null;
  loadingNode?: boolean;
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
  onOpenLink: (nodeId: string) => void;
  onOutlineNavigate?: () => void;
};

export function AuxiliarySidebar({
  activeNode,
  activeGroupId = null,
  loadingNode = false,
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
  onTextEncryptionEnabledChange,
  onOpenLink,
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
  const linksAvailable = activeNode !== null;
  const selectedView: MarkdownInspectorView = preferredView === "outline" && outlineAvailable
    ? "outline"
    : preferredView === "links" && linksAvailable
      ? "links"
      : "details";
  const linkIndex = useNodeLinkIndexQuery(selectedView === "links" ? activeNode : null);
  const syncLinkIndex = useSyncNodeLinkIndexMutation();
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
            { id: "outline", label: "Outline", controls: `${panelIdPrefix}-outline`, disabled: !outlineAvailable },
            { id: "links", label: "Links", controls: `${panelIdPrefix}-links`, disabled: !linksAvailable }
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
      <div
        id={`${panelIdPrefix}-links`}
        role="tabpanel"
        aria-labelledby={`${panelIdPrefix}-links-tab`}
        tabIndex={0}
        hidden={selectedView !== "links"}
        className="h-full overflow-y-auto p-3"
      >
        {activeNode ? (
          <LinksPanel
            node={activeNode}
            data={linkIndex.data}
            loading={linkIndex.isLoading}
            error={linkIndex.isError || syncLinkIndex.isError}
            syncing={syncLinkIndex.isPending}
            canSync={canWriteActiveSpace}
            onOpen={onOpenLink}
            onSync={() => syncLinkIndex.mutate({ spaceId: activeNode.space_id, nodeId: activeNode.id })}
          />
        ) : null}
      </div>
      </div>
    </aside>
  );
}

function LinksPanel({
  node,
  data,
  loading,
  error,
  syncing,
  canSync,
  onOpen,
  onSync
}: {
  node: RestNode;
  data?: NodeLinkIndexResponse;
  loading: boolean;
  error: boolean;
  syncing: boolean;
  canSync: boolean;
  onOpen: (nodeId: string) => void;
  onSync: () => void;
}) {
  return (
    <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
      <section className="p-4">
        <SectionHeader
          title="Link index"
          actions={node.kind === "text" ? (
            <Button secondary size="sm" onClick={onSync} disabled={!canSync || syncing}>
              <RefreshCw size={14} className={syncing ? "animate-spin" : undefined} />
              Sync now
            </Button>
          ) : undefined}
        />
        {loading ? <p className="text-xs text-muted">Loading links…</p> : null}
        {error ? <p role="alert" className="text-xs text-danger">Could not load links.</p> : null}
        {data ? (
          <dl className="space-y-2">
            <MetaRow label="Status" value={linkIndexStatusLabel(data.status)} />
            <MetaRow label="Last synced" value={formatLinkIndexSyncTime(data.last_synced_at)} />
          </dl>
        ) : null}
      </section>
      <LinkReferenceSection
        title="Outgoing"
        empty="This item does not link to another item."
        references={data?.outgoing ?? []}
        onOpen={onOpen}
      />
      <LinkReferenceSection
        title="Incoming"
        empty="No other item links here."
        references={data?.incoming ?? []}
        onOpen={onOpen}
      />
    </div>
  );
}

function LinkReferenceSection({
  title,
  empty,
  references,
  onOpen
}: {
  title: string;
  empty: string;
  references: LinkReference[];
  onOpen: (nodeId: string) => void;
}) {
  return (
    <section className="p-4">
      <SectionHeader title={title} actions={<span className="text-xs tabular-nums text-muted">{references.length}</span>} />
      {references.length === 0 ? <p className="text-xs text-muted">{empty}</p> : (
        <ul className="space-y-1">
          {references.map((reference) => {
            const missing = reference.node_id === null;
            return (
              <li key={`${reference.kind}:${reference.path}`}>
                <button
                  type="button"
                  className="flex min-h-9 w-full min-w-0 items-center gap-2 rounded-lg px-2 text-left text-sm hover:bg-[var(--ng-hover)] disabled:cursor-default disabled:hover:bg-transparent"
                  disabled={missing}
                  onClick={() => {
                    if (reference.node_id) onOpen(reference.node_id);
                  }}
                >
                  {missing ? <Unlink size={15} className="shrink-0 text-danger" /> : reference.kind === "image" ? (
                    <ImageIcon size={15} className="shrink-0 text-muted" />
                  ) : (
                    <Link2 size={15} className="shrink-0 text-muted" />
                  )}
                  <span className={`min-w-0 flex-1 truncate ${missing ? "text-danger" : "text-text"}`}>{reference.path}</span>
                  {missing ? <span className="shrink-0 text-xs text-danger">Missing</span> : null}
                  {reference.occurrence_count > 1 ? (
                    <span className="shrink-0 text-xs tabular-nums text-muted">×{reference.occurrence_count}</span>
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
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
