import { useInfiniteQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { queryKeys } from "../../api/queryKeys";
import { invalidateAuditEvents, invalidateFileSyncFallback, invalidateSpacesList } from "../../api/queryInvalidation";
import { listTrash, purgeTrash, restoreTrash, type TrashItem } from "../../api/trash";
import { Button, Modal } from "../../shared/ui";

export function TrashModal({ onClose }: { onClose: () => void }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const [confirmation, setConfirmation] = useState<TrashItem | null>(null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
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
        void queryClient.resetQueries({ queryKey: queryKeys.trash });
      }
    },
    onSuccess: async (_, { item, action }) => {
      setConfirmation(null);
      invalidateAuditEvents(queryClient);
      invalidateSpacesList(queryClient);
      void queryClient.invalidateQueries({ queryKey: queryKeys.usage });
      if (action === "restore") {
        void queryClient.invalidateQueries({ queryKey: queryKeys.space(item.space_id) });
        await invalidateFileSyncFallback(queryClient, item.space_id);
      }
      await queryClient.resetQueries({ queryKey: queryKeys.trash });
    }
  });
  const items = query.data?.pages.flatMap((page) => page.items) ?? [];
  const selected = items.find((item) => `${item.kind}:${item.id}` === selectedKey) ?? items[0];

  return (
    <Modal title="Trash" width="max-w-xl" onClose={onClose}>
      <p className="mb-3 text-muted">Deleted items are kept for 30 days.</p>
      {mutation.error ? <p role="alert" className="mb-3 text-danger">{mutation.error instanceof ApiError && mutation.error.status === 404 ? "This item is no longer available in this trash view." : mutation.error instanceof Error ? mutation.error.message : "Request failed"}</p> : null}
      {confirmation ? (
        <div className="space-y-4">
          <p>Permanently delete “{confirmation.name}”{confirmation.kind === "folder" || confirmation.kind === "space" ? " and everything inside it, including items deleted separately" : ""}? Recovery becomes unavailable immediately. Storage cleanup runs in the background.</p>
          <div className="flex justify-end gap-2">
            <Button secondary disabled={mutation.isPending} onClick={() => { mutation.reset(); setConfirmation(null); }}>Cancel</Button>
            <Button variant="danger" disabled={mutation.isPending} onClick={() => mutation.mutate({ item: confirmation, action: "purge" })}>
              {mutation.isPending ? "Requesting…" : "Permanently delete"}
            </Button>
          </div>
        </div>
      ) : (
        <>
          <div className="mb-3 flex justify-end">
            <Button secondary size="sm" disabled={query.isFetching || mutation.isPending} onClick={() => { void queryClient.resetQueries({ queryKey: queryKeys.trash }); }}>Refresh</Button>
          </div>
          {query.isPending ? <p role="status">Loading trash…</p> : null}
          {query.error ? <p role="alert" className="text-danger">Could not load trash. Try refreshing.</p> : null}
          {!query.isPending && !query.error && items.length === 0 ? <p className="text-muted">Trash is empty.</p> : null}
          <ul aria-label="Deleted items" className="max-h-[32vh] overflow-y-auto rounded-workbench-surface border border-seam empty:hidden">
            {items.map((item) => {
              const key = `${item.kind}:${item.id}`;
              const active = selected === item;
              return (
                <li key={key} className="border-b border-seam last:border-b-0">
                  <button
                    type="button"
                    aria-label={`Select ${item.name}`}
                    aria-pressed={active}
                    disabled={mutation.isPending}
                    onClick={() => { mutation.reset(); setSelectedKey(key); }}
                    className={`w-full px-3 py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45 disabled:cursor-wait ${active ? "bg-primary/15" : "hover:bg-[var(--ng-hover)]"}`}
                  >
                    <span className="block break-all font-medium">{item.name}</span>
                    <span className="block truncate text-muted" title={`${item.space_name} · ${item.path}`}>{item.space_name} · {item.path}</span>
                    <span className="block text-muted">Deleted {new Date(item.deleted_at).toLocaleDateString()} · {item.deletion_pending ? "Deletion queued" : item.recoverable ? "Recoverable" : "Restore unavailable"}</span>
                  </button>
                </li>
              );
            })}
          </ul>
          {query.hasNextPage ? <Button secondary className="mt-3" disabled={query.isFetchingNextPage || mutation.isPending} onClick={() => { void query.fetchNextPage(); }}>Load more</Button> : null}
          {selected ? (
            <section aria-label={`Details for ${selected.name}`} className="mt-3 border-t border-seam pt-3">
              <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
                <dt className="text-muted">Original location</dt><dd className="break-all">{selected.space_name} · {selected.path}</dd>
                <dt className="text-muted">Kind</dt><dd>{selected.kind}</dd>
                <dt className="text-muted">Deleted</dt><dd>{new Date(selected.deleted_at).toLocaleString()}</dd>
                <dt className="text-muted">Status</dt><dd>{selected.deletion_pending ? "Deletion queued · recovery unavailable" : `Kept until ${new Date(selected.purge_after).toLocaleString()}`}</dd>
              </dl>
              {!selected.deletion_pending ? (
                <p className="mt-2 text-muted">
                  {!selected.recoverable ? "Restore unavailable: parent deleted or content predates recovery support." : selected.kind === "space" ? "Restored spaces require reconnecting agents." : selected.kind === "folder" ? "Restores this folder and items from the same deletion. Items deleted separately stay in Trash. Name conflicts must be resolved first." : "Restores to the original location. Name conflicts must be resolved first."}
                </p>
              ) : null}
              <div className="mt-3 flex flex-wrap justify-end gap-2">
                <Button secondary size="sm" disabled={mutation.isPending || selected.deletion_pending || !selected.recoverable} aria-label={`Restore ${selected.name}`} onClick={() => mutation.mutate({ item: selected, action: "restore" })}>{mutation.isPending && mutation.variables?.action === "restore" ? "Restoring…" : "Restore"}</Button>
                <Button variant="danger" size="sm" disabled={mutation.isPending || selected.deletion_pending} aria-label={`Permanently delete ${selected.name}`} onClick={() => { mutation.reset(); setConfirmation(selected); }}>Delete permanently…</Button>
              </div>
            </section>
          ) : null}
        </>
      )}
    </Modal>
  );
}
