import { useState } from "react";
import { createPortal } from "react-dom";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { Button, Modal, Tabs } from "../../shared/ui";
import { TextPreview } from "../editor/TextPreview";
import { RevisionComparison } from "./RevisionComparison";
import { useTextRevisions } from "./useTextRevisions";

export default function TextRevisionModal({ node, canRestore, dirty, saving, onClose, onRestored }: {
  node: RestNode;
  canRestore: boolean;
  dirty: boolean;
  saving: boolean;
  onClose: () => void;
  onRestored: () => void;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [view, setView] = useState<"diff" | "full">("diff");
  const [confirming, setConfirming] = useState(false);
  const { list, revisions, selected, body, baseline, restore } = useTextRevisions(node, selectedId, onRestored);
  const current = baseline.data?.text;
  const plainCurrent = current?.storage_format === "plain" && "content" in current ? current : null;
  const unchanged = !!selected && selected.content_sha256 === current?.content_sha256;
  const externalChange = !!node.content_sha256 && !!plainCurrent && node.content_sha256 !== plainCurrent.content_sha256 && Date.parse(node.updated_at) > Date.parse(plainCurrent.updated_at);
  const conflict = restore.error instanceof ApiError && restore.error.status === 409;
  const blocked = !canRestore || dirty || saving || !selected || !body.isSuccess || body.isFetching || !baseline.isSuccess || baseline.isFetching || !plainCurrent || list.isError || restore.isPending || externalChange || conflict || unchanged;
  const showingCurrent = selectedId === "current";
  const head = list.data?.pages[0]?.current;
  const purpose = showingCurrent ? (head?.content_sha256 === current?.content_sha256 ? head?.purpose : null) : selected?.purpose;

  function select(id: string) {
    setSelectedId(id);
    setConfirming(false);
    // Do not clear a stale-hash error by switching historical versions; reload is required.
    if (!conflict) restore.reset();
  }
  async function reload() {
    setConfirming(false);
    const result = await baseline.refetch();
    if (!result.isError) restore.reset();
    void list.refetch();
  }
  function submit() {
    if (blocked || !selected || !current || !body.data) return;
    if (!confirming) { setConfirming(true); return; }
    restore.mutate({ revisionId: selected.id, sha: current.content_sha256, content: body.data.content });
  }
  const error = restore.error instanceof ApiError && restore.error.kind === "text_revision_storage_full"
    ? "Text history storage is full. Nothing was changed. Wait for retention cleanup or contact the administrator."
    : conflict ? "The current document changed. Reload the saved version and review the comparison before restoring."
      : restore.error instanceof ApiError && restore.error.status === 404 ? "This version is no longer available. Reload the version list."
        : restore.error instanceof Error ? restore.error.message : null;

  return createPortal(
    <Modal title={`Version history · ${node.name}`} width="max-w-6xl" onClose={() => { if (!restore.isPending) onClose(); }} footer={
      <div className="flex w-full flex-wrap items-center justify-between gap-3 border-t border-seam pt-4">
        <p className="max-w-xl text-xs text-muted" role="status">{confirming
          ? "Restore this entire version? The current saved content will remain in history."
          : "Comparing with the current saved version. Unsaved edits are not included."}</p>
        <div className="flex flex-wrap gap-2">
          <Button secondary disabled={restore.isPending} onClick={() => confirming ? setConfirming(false) : onClose()}>{confirming ? "Cancel" : "Close"}</Button>
          <Button disabled={blocked || showingCurrent} onClick={submit}>{restore.isPending ? "Restoring…" : confirming ? "Confirm restore" : "Restore this version"}</Button>
        </div>
      </div>
    }>
      <div className="flex h-[min(62dvh,42rem)] min-h-64 flex-col gap-4 md:flex-row">
        <nav aria-label="Saved versions" className="flex max-h-36 shrink-0 flex-col gap-1 overflow-y-auto border-b border-seam pb-2 md:max-h-none md:w-48 md:border-b-0 md:border-r md:pb-0 md:pr-3">
          <button type="button" disabled={restore.isPending} aria-pressed={showingCurrent} onClick={() => select("current")} className={`rounded-workbench px-3 py-2 text-left text-sm ${showingCurrent ? "bg-primary/15 text-text" : "text-muted hover:bg-[var(--ng-hover)]"}`}>
            Current saved version
          </button>
          <p className="px-3 py-2 text-xs text-muted">Previous versions</p>
          {list.isPending ? <p role="status" className="px-3 text-sm text-muted">Loading versions…</p> : null}
          {list.isError ? <div role="alert" className="px-3 text-sm text-danger">Could not load versions. <Button secondary size="sm" onClick={() => void list.refetch()}>Retry</Button></div> : null}
          {!list.isPending && !list.isError && !revisions.length ? <p className="px-3 text-sm text-muted">No previous versions yet. Versions appear after a changed save.</p> : null}
          {revisions.map((revision) => (
            <button key={revision.id} type="button" disabled={restore.isPending} aria-pressed={selected?.id === revision.id} onClick={() => select(revision.id)} className={`rounded-workbench border-l-2 px-3 py-2 text-left text-sm ${selected?.id === revision.id ? "border-primary bg-primary/15 text-text" : "border-transparent text-muted hover:bg-[var(--ng-hover)]"}`}>
              <time dateTime={revision.written_at}>{formatTime(revision.written_at)}</time>
              <span className="block text-xs text-muted">{sourceLabel(revision.source)}</span>
            </button>
          ))}
          {list.hasNextPage ? <Button secondary size="sm" disabled={list.isFetchingNextPage} onClick={() => void list.fetchNextPage()}>Load more</Button> : null}
        </nav>
        <section aria-label="Version content" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto md:overflow-visible">
          <Tabs items={[{ id: "diff", label: "Compare changes" }, { id: "full", label: "Full version" }]} value={view} onChange={setView} label="Version view" />
          <p aria-label="Change reason" className="mb-2 whitespace-pre-wrap break-words text-sm text-muted"><span className="font-medium">Change reason:</span> {purpose ?? "Not recorded"}</p>
          {dirty ? <p role="status" className="mb-2 text-sm text-warning">Unsaved edits are preserved. Close this window and save or cancel your edits before restoring.</p> : null}
          {!canRestore ? <p className="mb-2 text-sm text-muted">History is read-only. Restoring requires write access and an unlocked document.</p> : null}
          {error ? <p role="alert" className="mb-2 text-sm text-danger">{error}</p> : null}
          {externalChange && !conflict ? <p role="status" className="mb-2 text-sm text-warning">The saved document changed while this window was open.</p> : null}
          {externalChange || conflict || baseline.isError || restore.isError ? <div className="mb-2"><Button secondary size="sm" disabled={baseline.isFetching || restore.isPending} onClick={() => void reload()}>Reload saved version</Button></div> : null}
          {unchanged ? <p className="mb-2 text-sm text-muted">This version matches the current saved content.</p> : null}
          {baseline.isPending ? <p role="status" className="text-muted">Loading current saved version…</p>
            : baseline.isError ? <p role="alert" className="text-danger">Could not load the current saved version.</p>
              : !plainCurrent ? <p className="text-muted">History is unavailable for client-encrypted documents.</p>
                : showingCurrent ? <VersionPreview node={node} content={plainCurrent.content} identity="current" />
                  : body.isError || list.isError ? <p role="alert" className="text-danger">Could not read this version. It may have expired or access may have changed.</p>
                    : body.isFetching ? <p role="status" className="text-muted">Loading selected version…</p>
                      : body.data && selected ? <>
                        <p className="mb-2 text-xs text-muted">Selected: {formatTime(selected.written_at)} · Current: {formatTime(plainCurrent.updated_at)}</p>
                        {view === "full" ? <VersionPreview node={node} content={body.data.content} identity={selected.id} />
                          : plainCurrent.truncated ? <p className="text-muted">Current content is incomplete. Comparison is unavailable; use Full version.</p>
                            : <RevisionComparison key={`${selected.id}:${plainCurrent.content_sha256}`} before={body.data.content} after={plainCurrent.content} />}
                      </> : <p className="text-muted">Choose a version to view its content and changes.</p>}
        </section>
      </div>
    </Modal>, document.body
  );
}

function VersionPreview({ node, content, identity }: { node: RestNode; content: string; identity: string }) {
  const showToast = useUiStore((state) => state.showToast);
  return <div className="flex min-h-36 flex-1 shrink-0 overflow-auto rounded-workbench border border-seam md:min-h-0 md:shrink" onClickCapture={(event) => {
    if (event.target instanceof Element && event.target.closest("a[href]")) {
      event.preventDefault();
      event.stopPropagation();
      showToast("Links do not navigate from version previews. Copy the link address to open it separately.");
    }
  }}>
    <TextPreview name={node.name} content={content} previewIdentity={`revision:${node.id}:${identity}`} />
  </div>;
}
function formatTime(value: string) {
  return new Date(value).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", year: "numeric" });
}
function sourceLabel(source: string) {
  return ({ browser: "Edited on web", api: "Edited via API", mcp: "Edited via MCP", restore: "Restored version", unknown: "Saved version" } as Record<string, string>)[source] ?? "Saved version";
}
