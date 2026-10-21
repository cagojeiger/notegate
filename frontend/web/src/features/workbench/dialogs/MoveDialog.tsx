import { ChevronRight, Folder, FolderOpen, LockKeyhole } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button, Card, EmptyState, Modal } from "../../../shared/ui";
import { useNodeChildrenQuery } from "../../nodes/useNodeQueries";
import { dialogErrorMessage } from "./dialogErrors";
import type { AppDialog } from "./dialogTypes";

type Crumb = { id: string; name: string };

export function MoveDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "move" }>; onClose: () => void }) {
  const { node, space } = dialog;
  const [stack, setStack] = useState<Crumb[]>([{ id: space.root_node_id, name: "/" }]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const breadcrumbsRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const breadcrumbs = breadcrumbsRef.current;
    if (breadcrumbs) breadcrumbs.scrollLeft = breadcrumbs.scrollWidth;
  }, [stack]);
  const current = stack[stack.length - 1];
  const childrenQuery = useNodeChildrenQuery(space.id, current.id, true);
  // Only folders are valid destinations; never let the user descend into the
  // node being moved (that would also block reaching its descendants).
  const folders = useMemo(
    () => (childrenQuery.data?.pages ?? [])
      .flatMap((page) => page.children)
      .filter((child) => child.kind === "folder" && child.id !== node.id),
    [childrenQuery.data, node.id]
  );
  const alreadyHere = node.parent_id === current.id;
  const sourceLocked = node.effective_write_locked;
  async function moveHere() {
    if (alreadyHere || sourceLocked || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await dialog.onMove(current.id);
      onClose();
    } catch (moveError) {
      setError(dialogErrorMessage(moveError));
    } finally {
      setSubmitting(false);
    }
  }
  return (
    <Modal
      title={`Move "${node.name}"`}
      onClose={onClose}
      footer={
        <>
          <Button secondary onClick={onClose}>Cancel</Button>
          <Button onClick={() => void moveHere()} disabled={alreadyHere || sourceLocked || submitting}>Move here</Button>
        </>
      }
    >
      <div ref={breadcrumbsRef} role="navigation" aria-label="Destination path" className="flex min-h-workbench-control items-center gap-1 overflow-x-auto whitespace-nowrap p-1 text-xs text-muted">
        {stack.map((crumb, index) => (
          <span key={crumb.id} className="flex shrink-0 items-center gap-1">
            {index > 0 ? <ChevronRight size={12} className="text-faint" /> : null}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setStack((prev) => prev.slice(0, index + 1))}
              className={index === stack.length - 1 ? "font-semibold text-text" : undefined}
            >
              {crumb.name === "/" ? "Root" : crumb.name}
            </Button>
          </span>
        ))}
      </div>
      <Card padding="none" className="mt-3 h-[min(16rem,40dvh)] overflow-y-auto overscroll-contain p-1 [scrollbar-gutter:stable]">
        {childrenQuery.isLoading ? (
          <div className="px-3 py-2 text-sm text-muted">Loading…</div>
        ) : childrenQuery.isLoadingError ? (
          <div role="alert" className="flex min-h-[7rem] flex-col items-center justify-center gap-2 px-3 py-2 text-sm">
            <span className="text-danger">Could not load folders.</span>
            <Button size="sm" secondary onClick={() => void childrenQuery.refetch()}>
              Retry
            </Button>
          </div>
        ) : folders.length === 0 && !childrenQuery.hasNextPage ? (
          <EmptyState><span className="inline-flex items-center gap-2"><FolderOpen size={14} /> No subfolders here</span></EmptyState>
        ) : (
          <>
            {folders.map((folder) => (
              <button
                key={folder.id}
                type="button"
                onClick={() => setStack((prev) => [...prev, { id: folder.id, name: folder.name }])}
                disabled={folder.effective_write_locked}
                title={folder.effective_write_locked ? "Write locked" : undefined}
                className="flex min-h-workbench-control w-full items-center justify-between rounded-workbench-surface px-3 py-2 text-left text-workbench text-muted hover:bg-panel hover:text-text disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-muted"
              >
                <span className="flex min-w-0 items-center gap-2"><Folder size={14} className="shrink-0" /><span className="truncate">{folder.name}</span></span>
                {folder.effective_write_locked
                  ? <LockKeyhole size={14} className="shrink-0 text-faint" aria-hidden="true" />
                  : <ChevronRight size={14} className="shrink-0 text-faint" />}
              </button>
            ))}
            {childrenQuery.isFetchNextPageError ? (
              <p role="alert" className="px-3 py-2 text-center text-xs text-danger">Could not load more folders.</p>
            ) : null}
            {childrenQuery.hasNextPage ? (
              <Button
                className="w-full"
                variant="ghost"
                size="sm"
                disabled={childrenQuery.isFetchingNextPage}
                onClick={() => void childrenQuery.fetchNextPage()}
              >
                {childrenQuery.isFetchingNextPage ? "Loading…" : childrenQuery.isFetchNextPageError ? "Retry" : "Load more"}
              </Button>
            ) : null}
          </>
        )}
      </Card>
      <p className="mt-3 truncate text-xs text-muted" title={current.name}>Destination: <span className="font-mono text-text">{current.name === "/" ? "/" : current.name}</span>{alreadyHere ? " (already here)" : ""}</p>
      <div className="mt-2 h-12 overflow-y-auto overscroll-contain break-words text-xs text-danger">{error ? <p role="alert">{error}</p> : null}</div>
    </Modal>
  );
}
