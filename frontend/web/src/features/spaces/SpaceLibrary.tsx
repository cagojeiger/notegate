import { Bot, FolderOpen, LockKeyhole, Pin, RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { ApiError } from "../../api/errors";
import type { UpdateSpaceInput } from "../../api/spaces";
import type { Space } from "../../api/types";
import type { CurrentUserUsage, SpaceUsage } from "../../api/usage";
import { useLinkIndexStateQuery, useRequestLinkReindexMutation } from "../links/useLinkIndexQueries";
import { formatBytes } from "../../shared/lib/formatBytes";
import { WORKBENCH_LAYOUT } from "../../shared/model/workbenchLayout";
import { Button, Card, MetaRow, Modal, SectionHeader, SettingToggle } from "../../shared/ui";
import { SortableSpaceGrid } from "./SortableSpaceGrid";
import { useReorderSpacesMutation, useUpdateSpaceMutation } from "./useSpaceQueries";
import { useCheckSpaceUsageMutation, useUsageQuery } from "./useUsageQueries";

type SpaceLibraryProps = {
  spaces: Space[];
  activeSpace: Space | null;
  isMobile: boolean;
  usagePollingEnabled: boolean;
  inspectorOpen: boolean;
  onOpenInspector: () => void;
  onCloseInspector: () => void;
  onOpenSpace: (space: Space) => void;
  onCreateSpace: () => void;
};

type UsageLoadState = "loading" | "error" | "ready";

type UsageCheckProps = {
  disabled: boolean;
  error: Error | null;
  hasRequested: boolean;
  isRequesting: boolean;
  onCheck: () => void;
};

type SpaceInspectorProps = {
  space: Space | null;
  usage: SpaceUsage | undefined;
  usageState: UsageLoadState;
  usageFetching: boolean;
  pending: boolean;
  error: boolean;
  onRetryUsage: () => void;
  onUpdate: (input: UpdateSpaceInput) => void;
  usageCheck: UsageCheckProps;
  showHeader?: boolean;
};

export function SpaceLibrary({
  spaces,
  activeSpace,
  isMobile,
  usagePollingEnabled,
  inspectorOpen,
  onOpenInspector,
  onCloseInspector,
  onOpenSpace,
  onCreateSpace
}: SpaceLibraryProps) {
  const [selectedSpaceId, setSelectedSpaceId] = useState(activeSpace?.id ?? spaces[0]?.id ?? null);
  const usageQuery = useUsageQuery(usagePollingEnabled);
  const checkUsage = useCheckSpaceUsageMutation();
  const updateSpace = useUpdateSpaceMutation();
  const updateInspectorSpace = useUpdateSpaceMutation({ silentError: true });
  const reorderSpaces = useReorderSpacesMutation();
  const selectedSpace = spaces.find((space) => space.id === selectedSpaceId) ?? spaces[0] ?? null;
  const usageBySpaceId = useMemo(
    () => new Map((usageQuery.data?.spaces ?? []).map((usage) => [usage.id, usage])),
    [usageQuery.data?.spaces]
  );
  const selectedUsage = selectedSpace ? usageBySpaceId.get(selectedSpace.id) : undefined;
  const currentUsageState = usageState(usageQuery);
  const updatePending = updateSpace.isPending || updateInspectorSpace.isPending;
  const selectedCheckError = checkUsage.isError
    && checkUsage.variables === selectedSpace?.id
    && !(checkUsage.error instanceof ApiError && checkUsage.error.kind === "usage_reconciliation_pending")
    ? checkUsage.error
    : null;

  useEffect(() => {
    if (selectedSpaceId && spaces.some((space) => space.id === selectedSpaceId)) return;
    setSelectedSpaceId(activeSpace?.id ?? spaces[0]?.id ?? null);
  }, [activeSpace?.id, selectedSpaceId, spaces]);

  const updateSelectedSpace = (input: UpdateSpaceInput) => {
    if (!selectedSpace) return;
    updateInspectorSpace.mutate({ spaceId: selectedSpace.id, ...input });
  };
  const toggleNavigationPin = (space: Space) => {
    updateSpace.mutate({
      spaceId: space.id,
      navigation_pinned: !space.navigation_pinned
    });
  };
  const inspectSpace = (spaceId: string) => {
    setSelectedSpaceId(spaceId);
    onOpenInspector();
  };
  const inspectorProps: SpaceInspectorProps = {
    space: selectedSpace,
    usage: selectedUsage,
    usageState: currentUsageState,
    usageFetching: usageQuery.isFetching,
    pending: updatePending,
    error: updateInspectorSpace.isError,
    onRetryUsage: () => { void usageQuery.refetch(); },
    onUpdate: updateSelectedSpace,
    usageCheck: {
      disabled: checkUsage.isPending,
      error: selectedCheckError,
      hasRequested: checkUsage.variables === selectedSpace?.id && (checkUsage.isSuccess || checkUsage.isError),
      isRequesting: checkUsage.isPending && checkUsage.variables === selectedSpace?.id,
      onCheck: () => {
        if (!selectedSpace) return;
        checkUsage.reset();
        checkUsage.mutate(selectedSpace.id);
      }
    }
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">
      <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="h-12 shrink-0 border-b border-seam px-5 sm:px-7 lg:px-10">
          <div className="flex h-full w-full items-center justify-between gap-3">
            <h1 className="text-xl font-semibold">
              Spaces <span className="font-normal text-muted">{spaces.length}</span>
            </h1>
            <Button onClick={onCreateSpace}>Create space</Button>
          </div>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-6 sm:px-7 lg:px-10">
          <div className="w-full">
            {spaces.length === 0 ? (
              <Card className="grid min-h-56 place-items-center border-dashed text-center">
                <div>
                  <FolderOpen className="mx-auto text-muted" size={28} />
                  <h2 className="mt-3 font-semibold">No spaces yet</h2>
                  <p className="mt-1 text-sm text-muted">Create a space to start organizing your notes and files.</p>
                </div>
              </Card>
            ) : (
              <section aria-label="Spaces">
                <SortableSpaceGrid
                  spaces={spaces}
                  selectedSpaceId={selectedSpace?.id ?? null}
                  usageBySpaceId={usageBySpaceId}
                  updatePending={updatePending || reorderSpaces.isPending}
                  reorderPending={reorderSpaces.isPending || updatePending}
                  onSelect={inspectSpace}
                  onOpen={onOpenSpace}
                  onToggleNavigationPin={toggleNavigationPin}
                  onReorder={(orderedSpaces) => reorderSpaces.mutate({ spaces: orderedSpaces })}
                />
              </section>
            )}
          </div>
        </div>
      </section>

      {!isMobile && inspectorOpen ? (
        <aside
          aria-label="Space inspector"
          className="flex h-full min-h-0 shrink-0 overflow-hidden"
          style={{ width: WORKBENCH_LAYOUT.defaultAuxiliaryWidth }}
        >
          <SpaceInspector {...inspectorProps} />
        </aside>
      ) : null}

      {isMobile && inspectorOpen && selectedSpace ? (
        <Modal
          title="Space Inspector"
          placement="bottom"
          width="max-w-none"
          onClose={onCloseInspector}
        >
          <SpaceInspector {...inspectorProps} showHeader={false} />
        </Modal>
      ) : null}
    </div>
  );
}

function SpaceInspector({
  space,
  usage,
  usageState,
  usageFetching,
  pending,
  error,
  onRetryUsage,
  onUpdate,
  usageCheck,
  showHeader = true
}: SpaceInspectorProps) {
  const linkIndex = useLinkIndexStateQuery(space?.id ?? null);
  const requestLinkReindex = useRequestLinkReindexMutation();
  const isChecking = !!space && Boolean(usage?.reconciliation_pending || usageCheck.isRequesting);
  const isCooldown = usageCheck.error instanceof ApiError && usageCheck.error.kind === "usage_reconciliation_cooldown";
  const checkStatus = usageState === "ready" && usage
    ? isChecking
      ? { message: "Checking usage…", className: "text-warning" }
      : isCooldown
        ? { message: "Usage is already up to date.", className: "text-muted" }
        : usageCheck.error
          ? { message: "Usage could not be checked. Try again shortly.", className: "text-danger" }
          : usageCheck.hasRequested
            ? { message: "Usage is up to date.", className: "text-muted" }
            : null
    : null;
  const usageAction = space && usageState === "error"
    ? (
      <Button secondary size="sm" onClick={onRetryUsage} disabled={usageFetching} aria-label={`Retry ${space.name} usage`}>
        <RefreshCw size={14} className={usageFetching ? "animate-spin" : undefined} />
        Try again
      </Button>
    )
    : space && usage
      ? (
        <Button
          secondary
          size="sm"
          onClick={usageCheck.onCheck}
          disabled={isChecking || usageCheck.disabled}
          aria-label={`Check ${space.name} usage`}
        >
          <RefreshCw size={14} className={isChecking ? "animate-spin" : undefined} />
          {isChecking ? "Checking…" : "Check usage"}
        </Button>
      )
      : undefined;
  const linkReindexPending = Boolean(
    space
    && (
      (requestLinkReindex.isPending && requestLinkReindex.variables === space.id)
      || linkIndex.data?.freshness === "updating"
      || linkIndex.data?.freshness === "rebuilding"
    )
  );
  const linkAction = space ? (
    <Button
      secondary
      size="sm"
      onClick={() => {
        requestLinkReindex.reset();
        requestLinkReindex.mutate(space.id);
      }}
      disabled={space.permission !== "write" || linkReindexPending}
      aria-label={`Reindex links in ${space.name}`}
    >
      <RefreshCw size={14} className={linkReindexPending ? "animate-spin" : undefined} />
      {linkReindexPending ? "Reindexing…" : "Reindex links"}
    </Button>
  ) : undefined;

  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-panel md:border-l md:border-seam">
      {showHeader ? (
        <div className="flex h-12 shrink-0 items-center border-b border-seam px-3 text-sm font-medium">
          Space Inspector
        </div>
      ) : null}
      <div
        className="min-h-0 flex-1 overflow-y-auto p-3"
        data-testid="space-inspector-scroll-region"
      >
        <div className="divide-y divide-seam rounded-2xl border border-border bg-surface">
          <section className="p-4">
            <SectionHeader title="Space" />
            <dl className="space-y-2">
              <MetaRow label="Name" value={space?.name ?? "—"} />
              <MetaRow label="Permission" value={space?.permission ?? "—"} />
              <MetaRow label="Updated" value={space?.updated_at.slice(0, 10) ?? "—"} />
            </dl>
          </section>
          <section className="p-4">
            <SectionHeader
              title="Navigation"
              help="Pinned spaces stay visible in desktop and mobile navigation. Unpinned spaces remain available in the Space Library."
            />
            <SettingToggle
              icon={<Pin size={16} />}
              label="Pin to navigation"
              checked={space?.navigation_pinned ?? false}
              disabled={!space || pending}
              onChange={(checked) => onUpdate({ navigation_pinned: checked })}
            />
          </section>
          <section className="p-4">
            <SectionHeader
              title="Access"
              help="Controls whether User MCP can list and access this space. Agent MCP access is configured separately. Pinning does not affect MCP access."
            />
            <SettingToggle
              icon={<Bot size={16} />}
              label="User MCP access"
              checked={space?.user_mcp_enabled ?? false}
              disabled={!space || pending}
              onChange={(checked) => onUpdate({ user_mcp_enabled: checked })}
            />
          </section>
          <section className="p-4">
            <SectionHeader
              title="New item defaults"
              help="These settings apply only to new items created in this space. Search applies to every new item, while encryption applies only to new documents. Existing items are unchanged."
            />
            <div className="space-y-3">
              <SettingToggle
                icon={<Search size={16} />}
                label="Include in search"
                checked={space?.default_search_enabled ?? false}
                disabled={!space || pending}
                onChange={(checked) => onUpdate({ default_search_enabled: checked })}
              />
              <SettingToggle
                icon={<LockKeyhole size={16} />}
                label="Text encryption"
                badge={!space?.features.text_encryption ? "Max" : undefined}
                checked={space?.default_text_encryption_enabled ?? false}
                disabled={
                  !space
                  || pending
                  || (!space.features.text_encryption && !space.default_text_encryption_enabled)
                }
                onChange={(checked) => onUpdate({ default_text_encryption_enabled: checked })}
              />
            </div>
          </section>
          <section className="p-4">
            <SectionHeader
              title="Links"
              help="Indexes internal Markdown links for incoming, outgoing, and broken-link details. Reindexing runs in the background and does not block editing."
              actions={linkAction}
            />
            {!space ? <p className="text-sm text-muted">Select a space to inspect its link index.</p> : null}
            {space && linkIndex.isLoading ? <p className="text-sm text-muted">Loading link index…</p> : null}
            {space && linkIndex.isError ? <p className="text-sm text-danger">Could not load the link index.</p> : null}
            {linkIndex.data ? (
              <dl className="space-y-2">
                <MetaRow label="Status" value={formatLinkIndexStatus(linkIndex.data.freshness)} />
                <MetaRow
                  label="Last indexed"
                  value={formatLinkIndexTime(linkIndex.data.last_indexed_at)}
                />
              </dl>
            ) : null}
            {space && requestLinkReindex.isError && requestLinkReindex.variables === space.id ? (
              <p className="mt-3 text-xs text-danger" role="alert">Could not queue link reindexing.</p>
            ) : null}
          </section>
          <section className="p-4">
            <SectionHeader title="Usage" actions={usageAction} />
            {!space ? <p className="text-sm text-muted">Select a space to inspect it.</p> : null}
            {space && usageState === "loading" ? <p className="text-sm text-muted">Loading usage…</p> : null}
            {space && usageState === "error" ? <p className="text-sm text-danger">Could not load usage.</p> : null}
            {space && usageState === "ready" && !usage ? <p className="text-sm text-muted">Usage is not available.</p> : null}
            {usage ? <UsageRows usage={usage} /> : null}
            {checkStatus ? <p className={`mt-3 text-xs ${checkStatus.className}`} aria-live="polite">{checkStatus.message}</p> : null}
          </section>
          {error ? (
            <section role="alert" className="p-4 text-xs text-danger">Could not update this Space.</section>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function UsageRows({ usage }: { usage: SpaceUsage }) {
  return (
    <div className="space-y-3">
      <UsageRow label="Items" used={usage.items.used} limit={usage.items.limit} format={(value) => value.toLocaleString()} />
      <UsageRow label="Text" used={usage.text_bytes.used} limit={usage.text_bytes.limit} format={formatBytes} />
      <UsageRow label="Files" used={usage.file_bytes.used} limit={usage.file_bytes.limit} format={formatBytes} />
    </div>
  );
}

function UsageRow({ label, used, limit, format }: { label: string; used: number; limit: number; format: (value: number) => string }) {
  const percent = limit > 0 ? Math.min(100, (used / limit) * 100) : 0;
  const value = `${format(used)} / ${format(limit)}`;
  return (
    <div className="grid grid-cols-[auto_1fr] gap-x-3 text-xs">
      <span className="font-medium text-text">{label}</span>
      <span className="text-right text-muted">{value}</span>
      <div
        className="col-span-2 mt-1.5 h-1.5 overflow-hidden rounded-full bg-panel-strong"
        role="progressbar"
        aria-label={`${label} usage`}
        aria-valuemin={0}
        aria-valuemax={limit}
        aria-valuenow={Math.min(used, limit)}
        aria-valuetext={value}
      >
        <div className="h-full rounded-full bg-primary" style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

function usageState(query: { isLoading: boolean; isError: boolean; data?: CurrentUserUsage }): UsageLoadState {
  if (query.isLoading) return "loading";
  if (query.isError) return "error";
  return "ready";
}

function formatLinkIndexStatus(freshness: "current" | "updating" | "rebuilding" | "failed") {
  switch (freshness) {
    case "current": return "Current";
    case "updating": return "Updating";
    case "rebuilding": return "Rebuilding";
    case "failed": return "Update failed";
  }
}

function formatLinkIndexTime(value: string | null): string {
  if (!value) return "Not yet";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  }).format(date);
}
