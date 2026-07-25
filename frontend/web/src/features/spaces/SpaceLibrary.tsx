import { Bot, FolderOpen, LockKeyhole, Pin, Search } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";

import type { UpdateSpaceInput } from "../../api/spaces";
import type { Space } from "../../api/types";
import type { CurrentUserUsage, SpaceUsage } from "../../api/usage";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Badge, Button, Card, MetaRow, Modal, SectionHeader } from "../../shared/ui";
import { useUsageQuery } from "../settings/useUsageQueries";
import { SortableSpaceGrid } from "./SortableSpaceGrid";
import { useReorderSpacesMutation, useUpdateSpaceMutation } from "./useSpaceQueries";

type SpaceLibraryProps = {
  spaces: Space[];
  activeSpace: Space | null;
  isMobile: boolean;
  inspectorOpen: boolean;
  onOpenInspector: () => void;
  onCloseInspector: () => void;
  onOpenSpace: (space: Space) => void;
  onCreateSpace: () => void;
};

export function SpaceLibrary({
  spaces,
  activeSpace,
  isMobile,
  inspectorOpen,
  onOpenInspector,
  onCloseInspector,
  onOpenSpace,
  onCreateSpace
}: SpaceLibraryProps) {
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

  const updateSelectedSpace = (input: UpdateSpaceInput) => {
    if (!selectedSpace) return;
    updateSpace.mutate({ spaceId: selectedSpace.id, ...input });
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

  return (
    <div className="flex min-h-0 min-w-0 flex-1 bg-bg">
      <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="h-12 shrink-0 border-b border-seam px-5 sm:px-7 lg:px-10">
          <div className="mx-auto flex h-full max-w-7xl items-center justify-between gap-3">
            <h1 className="text-xl font-semibold">
              Spaces <span className="font-normal text-muted">{spaces.length}</span>
            </h1>
            <Button onClick={onCreateSpace}>Create space</Button>
          </div>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-6 sm:px-7 lg:px-10">
          <div className="mx-auto max-w-7xl">
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
                  updatePending={updateSpace.isPending || reorderSpaces.isPending}
                  reorderPending={reorderSpaces.isPending || updateSpace.isPending}
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
        <aside aria-label="Space inspector" className="w-80 shrink-0">
          <SpaceInspector
            space={selectedSpace}
            usage={selectedSpace ? usageBySpaceId.get(selectedSpace.id) : undefined}
            usageState={currentUsageState}
            pending={updateSpace.isPending}
            error={updateSpace.isError}
            onUpdate={updateSelectedSpace}
          />
        </aside>
      ) : null}

      {isMobile && inspectorOpen && selectedSpace ? (
        <Modal
          title="Space Inspector"
          placement="bottom"
          width="max-w-none"
          onClose={onCloseInspector}
        >
          <SpaceInspector
            space={selectedSpace}
            usage={usageBySpaceId.get(selectedSpace.id)}
            usageState={currentUsageState}
            pending={updateSpace.isPending}
            error={updateSpace.isError}
            onUpdate={updateSelectedSpace}
            showHeader={false}
          />
        </Modal>
      ) : null}
    </div>
  );
}

function SpaceInspector({
  space,
  usage,
  usageState,
  pending,
  error,
  onUpdate,
  showHeader = true
}: {
  space: Space | null;
  usage: SpaceUsage | undefined;
  usageState: "loading" | "error" | "ready";
  pending: boolean;
  error: boolean;
  onUpdate: (input: UpdateSpaceInput) => void;
  showHeader?: boolean;
}) {
  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-panel md:border-l md:border-seam">
      {showHeader ? (
        <div className="flex h-12 shrink-0 items-center border-b border-seam px-3 text-sm font-medium">
          Space Inspector
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
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
            <SectionHeader title="Navigation" />
            <SettingToggle
              icon={<Pin size={16} />}
              label="Pin to navigation"
              checked={space?.navigation_pinned ?? false}
              disabled={!space || pending}
              onChange={(checked) => onUpdate({ navigation_pinned: checked })}
            />
          </section>
          <section className="p-4">
            <SectionHeader title="Access" />
            <SettingToggle
              icon={<Bot size={16} />}
              label="User MCP access"
              checked={space?.user_mcp_enabled ?? false}
              disabled={!space || pending}
              onChange={(checked) => onUpdate({ user_mcp_enabled: checked })}
            />
          </section>
          <section className="p-4">
            <SectionHeader title="New item defaults" />
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
                disabled={!space || pending || !space.features.text_encryption}
                onChange={(checked) => onUpdate({ default_text_encryption_enabled: checked })}
              />
            </div>
          </section>
          <section className="p-4">
            <SectionHeader title="Usage" />
            {!space ? <p className="text-sm text-muted">Select a space to inspect it.</p> : null}
            {space && usageState === "loading" ? <p className="text-sm text-muted">Loading usage…</p> : null}
            {space && usageState === "error" ? <p className="text-sm text-danger">Could not load usage.</p> : null}
            {space && usageState === "ready" && !usage ? <p className="text-sm text-muted">Usage is not available.</p> : null}
            {usage ? <UsageRows usage={usage} /> : null}
          </section>
          {error ? (
            <section role="alert" className="p-4 text-xs text-danger">Could not update this Space.</section>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function SettingToggle({
  icon,
  label,
  badge,
  checked,
  disabled,
  onChange
}: {
  icon: ReactNode;
  label: string;
  badge?: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex min-h-8 items-center justify-between gap-3">
      <div className="flex min-w-0 items-center gap-2">
        <span className="grid size-6 shrink-0 place-items-center text-muted" aria-hidden="true">
          {icon}
        </span>
        <span className="text-sm font-medium text-text">{label}</span>
        {badge ? <Badge>{badge}</Badge> : null}
      </div>
      <button
        type="button"
        role="switch"
        aria-label={label}
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative h-6 w-10 shrink-0 rounded-full outline-none transition focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-40 ${
          checked ? "bg-primary" : "bg-panel-strong"
        }`}
      >
        <span
          className={`absolute top-0.5 size-5 rounded-full bg-white shadow-sm transition ${
            checked ? "left-[18px]" : "left-0.5"
          }`}
        />
      </button>
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
