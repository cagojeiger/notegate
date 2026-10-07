import { useInfiniteQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { queryKeys } from "../../api/queryKeys";
import { invalidateAuditEvents, invalidateFileSyncFallback, invalidateSpacesList } from "../../api/queryInvalidation";
import { listTrash, purgeTrash, restoreTrash, type TrashItem } from "../../api/trash";
import { Button, IconButton, Modal } from "../../shared/ui";

export function TrashModal({ onClose }: { onClose: () => void }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const descriptionId = useId();
  const detailsRef = useRef<HTMLDivElement>(null);
  const noticeRef = useRef<HTMLParagraphElement>(null);
  const returnFocus = useRef(false);
  const [confirmation, setConfirmation] = useState<TrashItem | null>(null);
  // undefined selects the initial item; null deliberately leaves no selection after restore.
  const [selectedKey, setSelectedKey] = useState<string | null>();
  const [notice, setNotice] = useState<string | null>(null);
  const query = useInfiniteQuery({
    queryKey: queryKeys.trash,
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => listTrash(client, pageParam),
    getNextPageParam: (last) => last.page.next_cursor ?? undefined
  });
  const mutation = useMutation({
    meta: { silentError: true },
    mutationFn: async ({ item, action }: { item: TrashItem; action: "restore" | "purge" }) => {
      if (action === "restore") await restoreTrash(client, item);
      else await purgeTrash(client, item);
    },
    onError: (error) => {
      if (error instanceof ApiError && (error.status === 404 || error.status === 409)) {
        setConfirmation(null);
        void queryClient.invalidateQueries({ queryKey: queryKeys.trash });
      }
    },
    onSuccess: async (_, { item, action }) => {
      setConfirmation(null);
      setNotice(action === "restore" ? `Restored “${item.name}” to ${item.space_name} · ${item.path}.` : `Permanent deletion requested for “${item.name}”. Recovery is unavailable; cleanup runs in the background.`);
      setSelectedKey(action === "restore" ? null : `${item.kind}:${item.id}`);
      invalidateAuditEvents(queryClient);
      invalidateSpacesList(queryClient);
      void queryClient.invalidateQueries({ queryKey: queryKeys.usage });
      if (action === "restore") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.space(item.space_id) });
        await invalidateFileSyncFallback(queryClient, item.space_id);
      }
      // Refetch the loaded pages in place, preserving pagination and the list's scroll container.
      await queryClient.invalidateQueries({ queryKey: queryKeys.trash });
    }
  });
  const items = query.data?.pages.flatMap((page) => page.items) ?? [];
  const selected = selectedKey === undefined ? items[0] : items.find((item) => `${item.kind}:${item.id}` === selectedKey);

  useEffect(() => {
    if (!confirmation && returnFocus.current) {
      returnFocus.current = false;
      detailsRef.current?.querySelector<HTMLButtonElement>("[data-trash-purge]")?.focus({ preventScroll: true });
    }
  }, [confirmation]);

  useEffect(() => {
    if (notice) noticeRef.current?.focus({ preventScroll: true });
  }, [notice]);

  return (
    <Modal title="Trash" width="max-w-3xl" onClose={onClose}>
      <p className="mb-3 text-muted">Deleted items are kept for 30 days.</p>
      {notice ? <p ref={noticeRef} role="status" tabIndex={-1} className="mb-3 break-words outline-none">{notice}</p> : null}
      {mutation.error ? <p role="alert" className="mb-3 text-danger">{mutation.error instanceof ApiError && mutation.error.status === 404 ? "This item is no longer available in this trash view." : mutation.error instanceof Error ? mutation.error.message : "Request failed"}</p> : null}
      <div className="grid gap-4 md:h-[min(56dvh,30rem)] md:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
        <div className="flex min-h-0 min-w-0 flex-col">
          <div className="mb-2 flex items-center justify-between">
            <h3 className="font-medium">Deleted items</h3>
            <IconButton label="Refresh" disabled={query.isFetching || mutation.isPending || !!confirmation} onClick={() => { void queryClient.invalidateQueries({ queryKey: queryKeys.trash }); }}><RefreshCw size={16} /></IconButton>
          </div>
          {query.isPending ? <p role="status">Loading trash…</p> : null}
          {query.error ? <p role="alert" className="text-danger">Could not load trash. Try refreshing.</p> : null}
          {!query.isPending && !query.error && items.length === 0 ? <p className="text-muted">Trash is empty.</p> : null}
          <ul aria-label="Deleted items" className="max-h-[28dvh] overflow-y-auto rounded-workbench-surface border border-seam empty:hidden md:min-h-0 md:max-h-none md:flex-1">
            {items.map((item) => {
              const key = `${item.kind}:${item.id}`;
              const itemDescriptionId = `${descriptionId}-${key}`;
              return (
                <li key={key} className="border-b border-seam last:border-b-0">
                  <button
                    type="button"
                    aria-label={`Select ${item.name}`}
                    aria-describedby={itemDescriptionId}
                    aria-pressed={selected === item}
                    disabled={mutation.isPending || !!confirmation}
                    onClick={() => { mutation.reset(); setNotice(null); setSelectedKey(key); }}
                    className={`w-full px-3 py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45 disabled:cursor-not-allowed ${selected === item ? "bg-primary/15" : "hover:bg-[var(--ng-hover)]"}`}
                  >
                    <span className="block break-all font-medium">{item.name}</span>
                    <span id={itemDescriptionId} className="block text-muted">
                      <span className="block truncate" title={`${item.space_name} · ${item.path}`}>{item.space_name} · {item.path}</span>
                      <span className="block">Deleted {new Date(item.deleted_at).toLocaleDateString()} · {item.deletion_pending ? "Deletion queued" : item.recoverable ? "Recoverable" : "Restore unavailable"}</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
          {query.hasNextPage ? <Button secondary className="mt-2 shrink-0" disabled={query.isFetching || mutation.isPending || !!confirmation} onClick={() => { void query.fetchNextPage(); }}>{query.isFetchingNextPage ? "Loading…" : "Load more"}</Button> : null}
        </div>
        <div ref={detailsRef} className="min-h-0 min-w-0 border-t border-seam pt-4 md:overflow-y-auto md:border-l md:border-t-0 md:pl-4 md:pt-0">
          {confirmation ? (
            <section aria-label="Confirm permanent deletion" className="flex h-full flex-col gap-3">
              <div id={`${descriptionId}-confirmation`}>
                <h3 className="font-medium">Permanently delete?</h3>
                <p className="mt-3 break-all font-medium">{confirmation.name}</p>
                <p className="mt-1 break-all text-muted">{confirmation.space_name} · {confirmation.path}</p>
                <p className="mt-3">{confirmation.kind === "folder" || confirmation.kind === "space" ? "Everything inside is included, even items deleted separately. " : ""}Recovery becomes unavailable immediately. Storage cleanup runs in the background.</p>
              </div>
              <div className="mt-auto flex flex-wrap justify-end gap-2 pt-3">
                <Button secondary autoFocus aria-describedby={`${descriptionId}-confirmation`} disabled={mutation.isPending} onClick={() => { mutation.reset(); returnFocus.current = true; setConfirmation(null); }}>Cancel</Button>
                <Button variant="danger" disabled={mutation.isPending} onClick={() => mutation.mutate({ item: confirmation, action: "purge" })}>
                  {mutation.isPending ? "Requesting…" : "Permanently delete"}
                </Button>
              </div>
            </section>
          ) : selected ? (
            <section aria-label={`Details for ${selected.name}`} className="flex h-full flex-col">
              <h3 className="break-all font-medium">{selected.name}</h3>
              <dl className="mt-3 space-y-3">
                <div><dt className="text-muted">Original location</dt><dd className="break-all">{selected.space_name} · {selected.path}</dd></div>
                <div><dt className="text-muted">{selected.deletion_pending ? "Status" : "Kept until"}</dt><dd>{selected.deletion_pending ? "Deletion queued · recovery unavailable" : new Date(selected.purge_after).toLocaleDateString()}</dd></div>
              </dl>
              {!selected.deletion_pending ? (
                <p className="mt-3 text-muted">
                  {!selected.recoverable ? "Restore unavailable: parent deleted or content predates recovery support." : selected.kind === "space" ? "Restored spaces require reconnecting agents." : selected.kind === "folder" ? "Restores this folder and items from the same deletion. Items deleted separately stay in Trash. Name conflicts must be resolved first." : "Restores to the original location. Name conflicts must be resolved first."}
                </p>
              ) : null}
              <div className="mt-auto flex flex-wrap items-center justify-between gap-2 pt-4">
                <Button variant="ghost" size="sm" data-trash-purge disabled={mutation.isPending || selected.deletion_pending} aria-label={`Permanently delete ${selected.name}`} onClick={() => { mutation.reset(); setNotice(null); setConfirmation(selected); }}>Delete permanently…</Button>
                <Button size="sm" disabled={mutation.isPending || selected.deletion_pending || !selected.recoverable} aria-label={`Restore ${selected.name}`} onClick={() => { setNotice(null); mutation.mutate({ item: selected, action: "restore" }); }}>{mutation.isPending && mutation.variables?.action === "restore" ? "Restoring…" : "Restore"}</Button>
              </div>
            </section>
          ) : <p className="text-muted">Select an item to restore or permanently delete.</p>}
        </div>
      </div>
    </Modal>
  );
}
