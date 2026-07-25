import { FolderOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import type { Space } from "../../api/types";
import type { CurrentUserUsage, SpaceUsage } from "../../api/usage";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Button, Card, MetaRow, SectionHeader } from "../../shared/ui";
import { useUsageQuery } from "../settings/useUsageQueries";
import { SortableSpaceGrid } from "./SortableSpaceGrid";
import { useReorderSpacesMutation, useUpdateSpaceMutation } from "./useSpaceQueries";

type SpaceLibraryProps = {
  spaces: Space[];
  activeSpace: Space | null;
  onOpenSpace: (space: Space) => void;
  onCreateSpace: () => void;
};

export function SpaceLibrary({ spaces, activeSpace, onOpenSpace, onCreateSpace }: SpaceLibraryProps) {
  const [selectedSpaceId, setSelectedSpaceId] = useState(activeSpace?.id ?? spaces[0]?.id ?? null);
  const usageQuery = useUsageQuery();
  const updateSpace = useUpdateSpaceMutation();
  const reorderSpaces = useReorderSpacesMutation();
  const selectedSpace = spaces.find((space) => space.id === selectedSpaceId) ?? spaces[0] ?? null;
  const usageBySpaceId = useMemo(
    () => new Map((usageQuery.data?.spaces ?? []).map((usage) => [usage.id, usage])),
    [usageQuery.data?.spaces]
  );
  const currentUsageState = usageState(usageQuery);

  useEffect(() => {
    if (selectedSpaceId && spaces.some((space) => space.id === selectedSpaceId)) return;
    setSelectedSpaceId(activeSpace?.id ?? spaces[0]?.id ?? null);
  }, [activeSpace?.id, selectedSpaceId, spaces]);

  const togglePin = (space: Space) => {
    updateSpace.mutate({ spaceId: space.id, pinned: !space.pinned });
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 bg-bg">
      <section className="min-w-0 flex-1 overflow-y-auto px-5 py-6 sm:px-7 lg:px-10">
        <div className="mx-auto max-w-7xl">
          <div className="mb-7 flex flex-wrap items-end justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Space Library</p>
              <h1 className="mt-1 text-2xl font-semibold tracking-tight">Your spaces</h1>
              <p className="mt-2 max-w-2xl text-sm leading-6 text-muted">
                Pinned spaces are available to your user MCP. Unpinned spaces stay private to the dashboard. Agent connections are unchanged.
              </p>
            </div>
            <Button onClick={onCreateSpace}>Create space</Button>
          </div>

          {spaces.length === 0 ? (
            <Card className="grid min-h-56 place-items-center border-dashed text-center">
              <div>
                <FolderOpen className="mx-auto text-muted" size={28} />
                <h2 className="mt-3 font-semibold">No spaces yet</h2>
                <p className="mt-1 text-sm text-muted">Create a space to start organizing your notes and files.</p>
              </div>
            </Card>
          ) : (
            <section aria-labelledby="space-library-all">
              <div className="mb-3">
                <h2 id="space-library-all" className="text-sm font-semibold">
                  All spaces <span className="font-normal text-muted">{spaces.length}</span>
                </h2>
                <p className="mt-1 text-xs text-muted">Drag to reorder. Pin controls user MCP access without changing the card position.</p>
              </div>
              <SortableSpaceGrid
                spaces={spaces}
                selectedSpaceId={selectedSpace?.id ?? null}
                usageBySpaceId={usageBySpaceId}
                pinPending={updateSpace.isPending || reorderSpaces.isPending}
                reorderPending={reorderSpaces.isPending || updateSpace.isPending}
                onSelect={setSelectedSpaceId}
                onOpen={onOpenSpace}
                onTogglePin={togglePin}
                onReorder={(orderedSpaces) => reorderSpaces.mutate({ spaces: orderedSpaces })}
              />
            </section>
          )}

          <div className="mt-8 md:hidden">
            <SpaceInspector
              space={selectedSpace}
              usage={selectedSpace ? usageBySpaceId.get(selectedSpace.id) : undefined}
              usageState={currentUsageState}
            />
          </div>
        </div>
      </section>

      <aside className="hidden w-80 shrink-0 border-l border-seam bg-panel p-3 md:block">
        <SpaceInspector
          space={selectedSpace}
          usage={selectedSpace ? usageBySpaceId.get(selectedSpace.id) : undefined}
          usageState={currentUsageState}
        />
      </aside>
    </div>
  );
}

function SpaceInspector({ space, usage, usageState }: { space: Space | null; usage: SpaceUsage | undefined; usageState: "loading" | "error" | "ready" }) {
  return (
    <div className="h-full w-full">
      <div className="rounded-xl bg-[var(--ng-hover)] px-3 py-1.5 text-sm font-medium">Space Inspector</div>
      <div className="mt-4 divide-y divide-seam rounded-2xl border border-border bg-surface">
        <section className="p-4">
          <SectionHeader title="Space" />
          <dl className="space-y-2">
            <MetaRow label="Name" value={space?.name ?? "—"} />
            <MetaRow label="MCP access" value={space ? (space.pinned ? "Pinned" : "Unpinned") : "—"} />
            <MetaRow label="Permission" value={space?.permission ?? "—"} />
            <MetaRow label="Updated" value={space?.updated_at.slice(0, 10) ?? "—"} />
          </dl>
        </section>
        <section className="p-4">
          <SectionHeader title="Usage" />
          {!space ? <p className="text-sm text-muted">Select a space to inspect it.</p> : null}
          {space && usageState === "loading" ? <p className="text-sm text-muted">Loading usage…</p> : null}
          {space && usageState === "error" ? <p className="text-sm text-danger">Could not load usage.</p> : null}
          {space && usageState === "ready" && !usage ? <p className="text-sm text-muted">Usage is not available.</p> : null}
          {usage ? <UsageRows usage={usage} /> : null}
        </section>
      </div>
    </div>
  );
}

function UsageRows({ usage }: { usage: SpaceUsage }) {
  return (
    <dl className="space-y-3">
      <UsageRow label="Items" used={usage.items.used} limit={usage.items.limit} format={(value) => value.toLocaleString()} />
      <UsageRow label="Text" used={usage.text_bytes.used} limit={usage.text_bytes.limit} format={formatBytes} />
      <UsageRow label="Files" used={usage.file_bytes.used} limit={usage.file_bytes.limit} format={formatBytes} />
      {usage.reconciliation_pending ? <p className="text-xs text-warning">Usage refresh in progress.</p> : null}
    </dl>
  );
}

function UsageRow({ label, used, limit, format }: { label: string; used: number; limit: number; format: (value: number) => string }) {
  const percent = limit > 0 ? Math.min(100, (used / limit) * 100) : 0;
  return (
    <div className="grid grid-cols-[auto_1fr] gap-x-3 text-xs">
      <dt className="font-medium text-text">{label}</dt>
      <dd className="text-right text-muted">{format(used)} / {format(limit)}</dd>
      <div className="col-span-2 mt-1.5 h-1.5 overflow-hidden rounded-full bg-panel-strong" aria-hidden="true">
        <div className="h-full rounded-full bg-primary" style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

function usageState(query: { isLoading: boolean; isError: boolean; data?: CurrentUserUsage }): "loading" | "error" | "ready" {
  if (query.isLoading) return "loading";
  if (query.isError) return "error";
  return "ready";
}
