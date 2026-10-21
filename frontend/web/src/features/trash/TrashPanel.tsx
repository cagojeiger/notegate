import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarDays, CircleAlert, Clock3, File, FileText, Folder, FolderOpen, Layers, RefreshCw, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { queryKeys } from "../../api/queryKeys";
import { invalidateAuditEvents, invalidateFileSyncFallback, invalidateSpacesList } from "../../api/queryInvalidation";
import { listTrash, purgeTrash, restoreTrash, type TrashItem } from "../../api/trash";
import { Button, Card, IconButton, ListItemButton, PanelSection } from "../../shared/ui";

import { SettingsControls, SettingsInspectorSection, useSettingsInspector } from "../settings/SettingsInspector";

import { SettingsPage } from "../settings/SettingsPage";
import { useSettingsScroll, useSettingsViewState } from "../settings/settingsViewState";
import { useSettingsMutation } from "../settings/useSettingsMutation";

const itemAppearance = {
  space: { icon: Layers, label: "Space" },
  folder: { icon: Folder, label: "Folder" },
  text: { icon: FileText, label: "Document" },
  file: { icon: File, label: "File" }
};

export function TrashPanel() {
  const inspector = useSettingsInspector();
  const client = useApiClient();
  const queryClient = useQueryClient();
  const descriptionId = useId();
  const detailsRef = useRef<HTMLDivElement>(null);
  const noticeRef = useRef<HTMLParagraphElement>(null);
  const returnFocus = useRef(false);
  const [confirmation, setConfirmation] = useState<TrashItem | null>(null);
  // undefined selects the initial item; null deliberately leaves no selection after restore.
  const [selectedKey, setSelectedKey] = useSettingsViewState<string | null | undefined>("selected-trash-item", undefined);
  const listScroll = useSettingsScroll<HTMLUListElement>("trash-list");
  const query = useInfiniteQuery({
    queryKey: queryKeys.trash,
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => listTrash(client, pageParam),
    getNextPageParam: (last) => last.page.next_cursor ?? undefined
  });
  const mutation = useSettingsMutation("trash", {
    meta: { silentError: true },
    mutationFn: async ({ item, action }: { item: TrashItem; action: "restore" | "purge" }) => {
      if (action === "restore") await restoreTrash(client, item);
      else await purgeTrash(client, item);
    },
    onError: (error) => {
      if (error instanceof ApiError && (error.status === 404 || error.status === 409)) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.trash });
      }
    },
    onSuccess: async (_, { item, action }) => {
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
  const completed = mutation.isSuccess ? mutation.variables : undefined;
  const notice = completed ? completed.action === "restore"
    ? `Restored “${completed.item.name}” to ${completed.item.space_name} · ${completed.item.path}.`
    : `Permanent deletion requested for “${completed.item.name}”. Recovery is unavailable; cleanup runs in the background.` : null;
  useEffect(() => {
    if (completed) {
      setConfirmation(null);
      setSelectedKey(completed.action === "restore" ? null : `${completed.item.kind}:${completed.item.id}`);
    } else if (mutation.error instanceof ApiError && [404, 409].includes(mutation.error.status)) setConfirmation(null);
  }, [completed, mutation.error, setSelectedKey]);
  const inspectorOpen = inspector?.open ?? true;
  useEffect(() => {
    // Dismiss unsubmitted confirmation; an in-flight request still owns its result.
    if (!inspectorOpen && !mutation.isPending) setConfirmation(null);
  }, [inspectorOpen, mutation.isPending]);
  const items = query.data?.pages.flatMap((page) => page.items) ?? [];
  const operation = mutation.variables;
  const effectiveKey = operation && (mutation.isPending || mutation.isSuccess)
    ? mutation.isSuccess && operation.action === "restore" ? null : `${operation.item.kind}:${operation.item.id}`
    : selectedKey;
  const selected = effectiveKey === undefined ? items[0] : items.find((item) => `${item.kind}:${item.id}` === effectiveKey);
  const SelectedIcon = itemAppearance[selected?.kind ?? "file"].icon;

  useEffect(() => {
    const scroller = inspector?.container ?? detailsRef.current;
    if (scroller) scroller.scrollTop = 0;
  }, [inspector?.container, selected?.kind, selected?.id]);

  useEffect(() => {
    if (!confirmation && returnFocus.current) {
      returnFocus.current = false;
      detailsRef.current?.querySelector<HTMLButtonElement>("[data-trash-purge]")?.focus();
    }
  }, [confirmation]);

  useEffect(() => {
    if (notice) noticeRef.current?.focus();
  }, [notice, inspector?.container]);

  return (
    <SettingsPage label="Trash" title="Deleted items" inspectorLabel="Open trash actions" scrollKey="trash" contained disabled={mutation.isPending}
      actions={<div className="flex items-center gap-2 text-xs text-muted"><span>Newest first</span><IconButton label="Refresh" disabled={query.isFetching || mutation.isPending || !!confirmation} onClick={() => { void queryClient.invalidateQueries({ queryKey: queryKeys.trash }); }}><RefreshCw size={16} aria-hidden="true" /></IconButton></div>}>
      <SettingsControls><PanelSection><p className="text-xs text-muted">Deleted items are kept for 30 days.</p></PanelSection></SettingsControls>
      {mutation.isPending ? <p role="status" className="text-xs text-muted">{operation?.action === "restore" ? "Restoring…" : "Requesting deletion…"}</p> : null}
      <div role="region" aria-label="Trash contents" className="flex min-h-0 flex-1 overflow-hidden">
      <div className="flex min-h-0 min-w-0 flex-1">
        <Card padding="none" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          {query.isPending ? <p role="status" className="p-3">Loading trash…</p> : null}
          {query.error ? <p role="alert" className="p-3 text-danger">Could not load trash. Try refreshing.</p> : null}
          {!query.isPending && !query.error && items.length === 0 ? <p className="p-3 text-muted">Trash is empty.</p> : null}
          {/* Keep absolutely positioned screen-reader text inside the list's scroll area. */}
          <ul ref={listScroll} aria-label="Deleted items" className="relative min-h-0 flex-1 divide-y divide-seam overflow-y-auto overscroll-contain empty:hidden">
            {items.map((item) => {
              const key = `${item.kind}:${item.id}`;
              const ItemIcon = itemAppearance[item.kind].icon;
              return (
                <li key={key}>
                  <ListItemButton
                    selected={selected === item}
                    disabled={mutation.isPending || !!confirmation}
                    onClick={() => { mutation.reset(); setSelectedKey(key); inspector?.onOpen(); }}
                    className="items-start"
                  >
                    <span className="sr-only">Select</span>{" "}
                    <ItemIcon size={16} aria-hidden="true" className={`mt-0.5 ${selected === item ? "text-primary" : "text-muted"}`} />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-baseline justify-between gap-2"><span className="truncate font-medium" title={item.name}>{item.name}</span>{" "}<time dateTime={item.deleted_at} title={`Deleted ${new Date(item.deleted_at).toLocaleString()}`} className="shrink-0 text-xs text-muted">{new Date(item.deleted_at).toLocaleDateString(undefined, { month: "short", day: "numeric" })}</time></span>{" "}
                      <span className="mt-1 block text-xs text-muted">
                        <span className="block truncate font-mono" title={`${item.space_name} · ${item.path}`}>{item.space_name} · {item.path}</span>{" "}
                        {item.deletion_pending ? <span className="mt-1 flex items-center gap-1"><Clock3 size={12} aria-hidden="true" />Deletion queued</span> : !item.recoverable ? <span className="mt-1 flex items-center gap-1 text-warning"><CircleAlert size={12} aria-hidden="true" />Restore unavailable</span> : <span className="sr-only">Recoverable</span>}
                        {" "}<span className="sr-only">Deleted {new Date(item.deleted_at).toLocaleString()}.</span>
                      </span>
                    </span>
                  </ListItemButton>
                </li>
              );
            })}
          </ul>
          {query.hasNextPage ? <Button secondary className="m-3 shrink-0" disabled={query.isFetching || mutation.isPending || !!confirmation} onClick={() => { void query.fetchNextPage(); }}>{query.isFetchingNextPage ? "Loading…" : "Load more"}</Button> : null}
        </Card>
        <SettingsControls><SettingsInspectorSection title="Selected item"><div ref={detailsRef} className="min-h-0 min-w-0 text-sm">
          {notice ? <p ref={noticeRef} role="status" tabIndex={-1} className="mb-3 break-words outline-none">{notice}</p> : null}
          {mutation.error ? <p role="alert" className="mb-3 break-words text-danger">{mutation.error instanceof ApiError && mutation.error.status === 404 ? "This item is no longer available in this trash view." : mutation.error instanceof Error ? mutation.error.message : "Request failed"}</p> : null}
          {confirmation ? (
            <section aria-label="Confirm permanent deletion" className="flex h-full flex-col gap-3">
              <div id={`${descriptionId}-confirmation`}>
                <h3 className="flex items-center gap-2 text-base font-semibold"><Trash2 size={18} aria-hidden="true" />Permanently delete?</h3>
                <p className="mt-3 break-all font-medium">{confirmation.name}</p>
                <p className="mt-1 break-all text-muted">{confirmation.space_name} · {confirmation.path}</p>
                <p className="mt-3">{confirmation.kind === "folder" || confirmation.kind === "space" ? "Everything inside is included, even items deleted separately. " : ""}Recovery becomes unavailable immediately. Storage cleanup runs in the background.</p>
              </div>
              <div className="mt-auto flex flex-wrap justify-end gap-2 border-t border-seam pt-4">
                <Button secondary autoFocus aria-describedby={`${descriptionId}-confirmation`} disabled={mutation.isPending} onClick={() => { mutation.reset(); returnFocus.current = true; setConfirmation(null); }}>Cancel</Button>
                <Button variant="danger" disabled={mutation.isPending} onClick={() => mutation.mutate({ item: confirmation, action: "purge" })}>
                  {mutation.isPending ? "Requesting…" : "Permanently delete"}
                </Button>
              </div>
            </section>
          ) : selected ? (
            <section aria-label={`Details for ${selected.name}`} className="flex h-full flex-col">
              <div className="flex items-center gap-3"><SelectedIcon size={24} aria-hidden="true" className="text-primary" /><div className="min-w-0"><h3 className="break-all text-base font-semibold">{selected.name}</h3><p className="mt-1 text-xs text-muted">{itemAppearance[selected.kind].label} · {selected.space_name}</p></div></div>
              <dl className="mt-5 space-y-4">
                <div><dt className="mb-1 text-xs text-muted">Original location</dt><dd className="flex items-start gap-2"><FolderOpen size={14} aria-hidden="true" className="mt-0.5 text-muted" /><span className="break-all font-mono text-xs">{selected.space_name} · {selected.path}</span></dd></div>
                <div><dt className="mb-1 text-xs text-muted">{selected.deletion_pending ? "Status" : "Kept until"}</dt><dd className="flex items-start gap-2"><CalendarDays size={14} aria-hidden="true" className="mt-0.5 text-muted" /><span>{selected.deletion_pending ? "Deletion queued · recovery unavailable" : new Date(selected.purge_after).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" })}</span></dd></div>
              </dl>
              {!selected.deletion_pending ? (
                <Card padding="sm" className="my-4">
                  <p className="mb-1 font-medium">{!selected.recoverable ? "Restore unavailable" : selected.kind === "folder" ? "What will be restored" : "Restore to original location"}</p>
                  <p className="text-xs text-muted">{!selected.recoverable ? "The parent is deleted or this content predates recovery support." : selected.kind === "space" ? "Restored spaces require reconnecting agents." : selected.kind === "folder" ? "Items deleted with this folder are included. Items deleted separately stay in Trash. Name conflicts must be resolved first." : "Existing files will not be overwritten. Resolve any name conflict before restoring."}</p>
                </Card>
              ) : null}
              <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-seam pt-4">
                <Button variant="danger" size="sm" data-trash-purge disabled={mutation.isPending || selected.deletion_pending} aria-label={`Permanently delete ${selected.name}`} onClick={() => { mutation.reset(); setConfirmation(selected); }}>Permanently delete</Button>
                <Button size="sm" disabled={mutation.isPending || selected.deletion_pending || !selected.recoverable} aria-label={`Restore ${selected.name}`} onClick={() => mutation.mutate({ item: selected, action: "restore" })}><RotateCcw size={14} aria-hidden="true" />{mutation.isPending && mutation.variables?.action === "restore" ? "Restoring…" : "Restore"}</Button>
              </div>
            </section>
          ) : <p className="text-muted">Select an item to restore or permanently delete.</p>}
        </div></SettingsInspectorSection></SettingsControls>
      </div>
      </div>
    </SettingsPage>
  );
}
