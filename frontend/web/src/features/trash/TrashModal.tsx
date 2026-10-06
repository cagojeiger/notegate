import { useInfiniteQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { queryKeys } from "../../api/queryKeys";
import { invalidateAuditEvents, invalidateFileSyncFallback, invalidateSpacesList } from "../../api/queryInvalidation";
import { listTrash, purgeTrash, restoreTrash, type TrashItem } from "../../api/trash";
import { Button, Modal } from "../../shared/ui";

export function TrashModal({ onClose }: { onClose: () => void }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const [confirmation, setConfirmation] = useState<TrashItem | null>(null);
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
    onSuccess: async (_, { item, action }) => {
      setConfirmation(null);
      invalidateAuditEvents(queryClient);
      invalidateSpacesList(queryClient);
      void queryClient.invalidateQueries({ queryKey: queryKeys.usage });
      if (action === "restore" && item.kind !== "space") {
        await invalidateFileSyncFallback(queryClient, item.space_id);
      }
      await queryClient.resetQueries({ queryKey: queryKeys.trash });
    }
  });
  const items = query.data?.pages.flatMap((page) => page.items) ?? [];

  return (
    <Modal title="Trash" width="max-w-3xl" onClose={onClose}>
      <p className="mb-4 text-muted">
        Deleted items are kept for 30 days. Restore returns them to their existing location;
        name conflicts must be resolved first. Restore a deleted parent before its children.
        Restored spaces require reconnecting agents.
      </p>
      {mutation.error ? <p role="alert" className="mb-3 text-danger">{mutation.error instanceof Error ? mutation.error.message : "Request failed"}</p> : null}
      {confirmation ? (
        <div className="space-y-4">
          <p>Permanently delete “{confirmation.name}”{confirmation.kind === "folder" || confirmation.kind === "space" ? " and everything inside it" : ""}? Recovery becomes unavailable immediately. Storage cleanup runs in the background.</p>
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
          <ul className="max-h-[55vh] space-y-2 overflow-y-auto">
            {items.map((item) => {
              const queued = item.deletion_pending;
              return (
                <li key={`${item.kind}:${item.id}`} className="rounded-workbench-surface border border-seam p-3">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div className="min-w-0 flex-1">
                      <p className="break-all font-medium">{item.name}</p>
                      <p className="break-all text-muted">{item.space_name} · {item.path} · {item.kind}</p>
                      <p className="mt-1 text-muted">Deleted {new Date(item.deleted_at).toLocaleString()}</p>
                      <p className="text-muted">{queued ? "Deletion queued · recovery unavailable" : `Kept until ${new Date(item.purge_after).toLocaleString()}`}</p>
                      {!queued && !item.recoverable ? <p className="text-muted">Restore unavailable: parent deleted or content predates recovery support.</p> : null}
                    </div>
                    <div className="flex justify-end gap-2">
                      <Button secondary size="sm" disabled={mutation.isPending || queued || !item.recoverable} aria-label={`Restore ${item.name}`} onClick={() => mutation.mutate({ item, action: "restore" })}>Restore</Button>
                      <Button variant="danger" size="sm" disabled={mutation.isPending || queued} aria-label={`Permanently delete ${item.name}`} onClick={() => { mutation.reset(); setConfirmation(item); }}>Delete permanently</Button>
                    </div>
                  </div>
                </li>
              );
            })}
          </ul>
          {query.hasNextPage ? <Button secondary className="mt-3" disabled={query.isFetchingNextPage || mutation.isPending} onClick={() => { void query.fetchNextPage(); }}>Load more</Button> : null}
        </>
      )}
    </Modal>
  );
}
