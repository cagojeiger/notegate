import { useId } from "react";
import { CircleHelp, RefreshCw } from "lucide-react";

import { ApiError } from "../../api/errors";
import type { QuotaUsage, SpaceUsage } from "../../api/usage";
import { cn } from "../../shared/lib/cn";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Badge, Button, Card, EmptyState, SectionHeader } from "../../shared/ui";
import { useCheckSpaceUsageMutation, useUsageQuery } from "./useUsageQueries";

const numberFormatter = new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 });

export function UsageTab() {
  const usageQuery = useUsageQuery();
  const checkMutation = useCheckSpaceUsageMutation();

  if (usageQuery.isPending) {
    return <div className="text-sm text-muted" aria-live="polite">Loading usage…</div>;
  }

  if (usageQuery.isError || !usageQuery.data) {
    return (
      <Card tone="danger" className="flex flex-wrap items-center justify-between gap-3 text-sm">
        <span>Usage is unavailable right now.</span>
        <Button secondary size="sm" onClick={() => { void usageQuery.refetch(); }} disabled={usageQuery.isFetching}>
          <RefreshCw size={14} className={usageQuery.isFetching ? "animate-spin" : undefined} />
          Try again
        </Button>
      </Card>
    );
  }

  const usage = usageQuery.data;
  return (
    <section aria-busy={usageQuery.isFetching}>
      <SectionHeader title="Space usage" actions={<Badge>{formatTier(usage.tier)}</Badge>} />
      {usage.spaces.length === 0 ? (
        <EmptyState>No spaces yet.</EmptyState>
      ) : (
        <ul className="space-y-3">
          {usage.spaces.map((space) => (
            <SpaceUsageCard
              key={space.id}
              space={space}
              isRequesting={checkMutation.isPending && checkMutation.variables === space.id}
              requestDisabled={checkMutation.isPending}
              requestError={checkMutation.isError
                && checkMutation.variables === space.id
                && !(checkMutation.error instanceof ApiError && checkMutation.error.kind === "usage_reconciliation_pending")
                ? checkMutation.error
                : null}
              onCheck={() => {
                checkMutation.reset();
                checkMutation.mutate(space.id);
              }}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function SpaceUsageCard({
  space,
  isRequesting,
  requestDisabled,
  requestError,
  onCheck
}: {
  space: SpaceUsage;
  isRequesting: boolean;
  requestDisabled: boolean;
  requestError: Error | null;
  onCheck: () => void;
}) {
  const isChecking = space.reconciliation_pending || isRequesting;
  const isCooldown = requestError instanceof ApiError && requestError.kind === "usage_reconciliation_cooldown";
  const status = isChecking
    ? "Checking usage…"
    : isCooldown
      ? "Usage is already up to date."
      : requestError
        ? "Usage could not be checked. Try again shortly."
        : "Usage is up to date.";

  return (
    <Card as="li" padding="none">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-seam px-4 py-3">
        <div className="min-w-0">
          <p className="text-xs text-muted">Space</p>
          <h4 className="truncate text-sm font-semibold">{space.name}</h4>
          <p className={cn("mt-0.5 text-xs", requestError && !isCooldown ? "text-danger" : "text-muted")} aria-live="polite">
            {status}
          </p>
        </div>
        <Button
          secondary
          size="sm"
          className="shrink-0"
          onClick={onCheck}
          disabled={isChecking || requestDisabled}
          aria-label={`Check ${space.name} usage`}
        >
          <RefreshCw size={14} className={isChecking ? "animate-spin" : undefined} />
          {isChecking ? "Checking…" : "Check usage"}
        </Button>
      </div>
      <div className="grid divide-y divide-seam sm:grid-cols-3 sm:divide-x sm:divide-y-0">
        <UsageMeter
          label="Text storage"
          description="Text content currently stored in this space. Deleted items and metadata are excluded."
          usage={space.text_bytes}
          format="bytes"
        />
        <UsageMeter
          label="File storage"
          description="File content currently stored in this space. Deleted items are excluded."
          usage={space.file_bytes}
          format="bytes"
        />
        <UsageMeter
          label="Items"
          description="Folders, Text, and Files currently in this space."
          usage={space.items}
          helpAlign="end"
        />
      </div>
    </Card>
  );
}

function UsageMeter({
  label,
  description,
  usage,
  format = "count",
  helpAlign = "start"
}: {
  label: string;
  description: string;
  usage: QuotaUsage;
  format?: "count" | "bytes";
  helpAlign?: "start" | "end";
}) {
  const descriptionId = useId();
  const percentage = usage.limit > 0 ? Math.min(100, (usage.used / usage.limit) * 100) : 0;
  const fillClass = percentage >= 100 ? "bg-danger" : percentage >= 80 ? "bg-warning" : "bg-primary";
  const value = format === "bytes"
    ? `${formatBytes(usage.used)} / ${formatBytes(usage.limit)}`
    : `${numberFormatter.format(usage.used)} / ${numberFormatter.format(usage.limit)}`;

  return (
    <div className="relative min-w-0 p-3 sm:p-4">
      <div className="flex items-baseline justify-between gap-3 text-xs sm:block">
        <span className="inline-flex items-center gap-1 font-medium text-text">
          {label}
          <button
            type="button"
            className="peer inline-grid size-5 shrink-0 place-items-center rounded text-muted outline-none hover:bg-panel-strong hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45"
            aria-label={`About ${label}`}
            aria-describedby={descriptionId}
          >
            <CircleHelp size={13} />
          </button>
          <span
            id={descriptionId}
            role="tooltip"
            className={cn(
              "pointer-events-none invisible absolute left-3 right-3 top-10 z-20 rounded-md border border-border bg-panel px-2.5 py-2 text-xs font-normal leading-5 text-text opacity-0 shadow-lg transition-opacity peer-hover:visible peer-hover:opacity-100 peer-focus:visible peer-focus:opacity-100 sm:w-56",
              helpAlign === "end" ? "sm:left-auto sm:right-4" : "sm:left-4 sm:right-auto"
            )}
          >
            {description}
          </span>
        </span>
        <span className="whitespace-nowrap font-mono tabular-nums text-muted sm:mt-1 sm:block" title={value}>{value}</span>
      </div>
      <div
        className="mt-2 h-1.5 overflow-hidden rounded-full bg-panel-strong"
        role="progressbar"
        aria-label={`${label} usage`}
        aria-valuemin={0}
        aria-valuemax={usage.limit}
        aria-valuenow={Math.min(usage.used, usage.limit)}
        aria-valuetext={value}
      >
        <div className={cn("h-full rounded-full transition-[width]", fillClass)} style={{ width: `${percentage}%` }} />
      </div>
    </div>
  );
}

function formatTier(tier: string): string {
  if (tier === "tier0") return "Tier 0";
  if (tier === "system_max") return "System max";
  return tier.split("_").join(" ");
}
