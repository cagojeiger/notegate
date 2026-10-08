import { Bot, Link2, LockKeyhole, Pin, RefreshCw, Plug } from "lucide-react";
import { useEffect, useState } from "react";

import type { UpdateSpaceInput } from "../../api/spaces";
import type { Space } from "../../api/types";
import type { SpaceUsage } from "../../api/usage";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Button, MetaRow, SectionHeader, SettingToggle } from "../../shared/ui";
import {
  useReindexSpaceLinksMutation,
  useSpaceLinkIndexStatusQuery
} from "../links/useLinkQueries";

type UsageLoadState = "loading" | "error" | "ready";

type UsageCheckProps = {
  disabled: boolean;
  error: Error | null;
  hasRequested: boolean;
  isRequesting: boolean;
  onCheck: () => void;
};

export type SpaceInspectorProps = {
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

export function SpaceInspector({
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
  const reindexLinks = useReindexSpaceLinksMutation();
  const canReindexLinks = space?.permission === "write";
  const linkIndexStatus = useSpaceLinkIndexStatusQuery(space?.id, canReindexLinks);
  const isChecking = !!space && Boolean(
    usage?.reconciliation?.status === "pending" || usageCheck.isRequesting
  );
  const usageAvailability = usage?.reconciliation?.availability;
  const reconciliationAvailableAt = usageAvailability?.retry_at ?? undefined;
  const retryInFuture = useTimestampInFuture(reconciliationAvailableAt);
  const isCooldown = usageAvailability?.reason === "cooldown" && retryInFuture;
  const canCheckUsage = usageAvailability?.can_trigger === true
    || (usageAvailability?.reason === "cooldown" && !isCooldown);
  const checkStatus = usageState === "ready" && usage
    ? isChecking
      ? { message: "Recalculating usage…", className: "text-warning" }
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
      <Button variant="secondary" size="xs" onClick={onRetryUsage} disabled={usageFetching} aria-label={`Retry ${space.name} usage`}>
        <RefreshCw size={14} className={usageFetching ? "animate-spin" : undefined} />
        Try again
      </Button>
    )
    : space && usage
      ? (
        <Button
          variant="secondary"
          size="xs"
          onClick={usageCheck.onCheck}
          disabled={isChecking || !canCheckUsage || usageCheck.disabled}
          title={isCooldown && reconciliationAvailableAt
            ? `Available after ${formatAvailability(reconciliationAvailableAt)}`
            : undefined}
          aria-label={`Recalculate ${space.name} usage`}
        >
          <RefreshCw size={14} className={isChecking ? "animate-spin" : undefined} />
          {isChecking ? "Recalculating…" : "Recalculate"}
        </Button>
      )
      : undefined;
  const reindexForCurrentSpace = Boolean(space && reindexLinks.variables === space.id);
  const reindexPending = reindexForCurrentSpace && reindexLinks.isPending;
  const linkIndexPending = canReindexLinks && Boolean(
    linkIndexStatus.data?.status === "pending" || reindexPending
  );
  const linkIndexUnavailable = linkIndexStatus.isLoading
    || linkIndexStatus.isError
    || linkIndexStatus.data?.availability?.can_trigger !== true
    || reindexPending;

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
              help="These settings apply only to new items created in this space. MCP & API access applies to every new item, while encryption applies only to new documents. Existing items are unchanged."
            />
            <div className="space-y-3">
              <SettingToggle
                icon={<Plug size={16} />}
                label="MCP & API access"
                checked={space?.default_external_access_enabled ?? false}
                disabled={!space || pending}
                onChange={(checked) => onUpdate({ default_external_access_enabled: checked })}
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
              title="Link index"
              help="Rebuilds Markdown link relationships for this Space in the background."
              actions={space ? (
                <Button
                  variant="secondary"
                  size="xs"
                  disabled={!canReindexLinks || linkIndexUnavailable}
                  onClick={() => reindexLinks.mutate(space.id)}
                  aria-label={`Reindex links in ${space.name}`}
                >
                  <Link2 size={14} />
                  {linkIndexPending ? "Reindexing…" : "Reindex"}
                </Button>
              ) : undefined}
            />
            {linkIndexPending ? (
              <p className="text-xs text-muted" role="status">Link indexing is in progress.</p>
            ) : reindexForCurrentSpace && reindexLinks.isSuccess ? (
              <p className="text-xs text-muted" role="status">Link index is up to date.</p>
            ) : null}
            {canReindexLinks && linkIndexStatus.isError ? (
              <p className="text-xs text-danger" role="alert">Could not load link index status.</p>
            ) : reindexForCurrentSpace && reindexLinks.isError ? (
              <p className="text-xs text-danger" role="alert">Could not request link reindex.</p>
            ) : null}
          </section>
          <section className="p-4">
            <SectionHeader title="Usage" />
            {!space ? <p className="text-sm text-muted">Select a space to inspect it.</p> : null}
            {space && usageState === "loading" ? <p className="text-sm text-muted">Loading usage…</p> : null}
            {space && usageState === "error" ? <p className="text-sm text-danger">Could not load usage.</p> : null}
            {space && usageState === "ready" && !usage ? <p className="text-sm text-muted">Usage is not available.</p> : null}
            {usage ? <UsageRows usage={usage} /> : null}
            {usageAction || checkStatus ? (
              <div className={`mt-3 flex items-center gap-2 ${checkStatus ? "justify-between" : "justify-end"}`}>
                {checkStatus ? <p className={`text-xs ${checkStatus.className}`} aria-live="polite">{checkStatus.message}</p> : null}
                {usageAction}
              </div>
            ) : null}
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
      {(usage.retained_text_bytes ?? 0) > 0 || (usage.retained_file_bytes ?? 0) > 0 ? (
        <p className="text-xs text-muted">
          Includes retained content: {formatBytes(usage.retained_text_bytes ?? 0)} text and {formatBytes(usage.retained_file_bytes ?? 0)} files.
          Trash and files awaiting deletion still count toward these limits.
        </p>
      ) : null}
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

function useTimestampInFuture(timestamp: string | undefined): boolean {
  const [, setExpired] = useState(false);
  const availableAt = timestamp ? Date.parse(timestamp) : Number.NaN;
  const isFuture = Number.isFinite(availableAt) && availableAt > Date.now();

  useEffect(() => {
    if (!isFuture) return;
    const timeout = window.setTimeout(
      () => setExpired((value) => !value),
      Math.min(availableAt - Date.now(), 2_147_483_647)
    );
    return () => window.clearTimeout(timeout);
  }, [availableAt, isFuture]);

  return isFuture;
}

function formatAvailability(timestamp: string): string {
  const value = new Date(timestamp);
  return Number.isNaN(value.getTime()) ? timestamp : value.toLocaleString();
}
