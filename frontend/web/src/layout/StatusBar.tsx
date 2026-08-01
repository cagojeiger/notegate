import { HardDrive, ListTree, LockKeyhole, RefreshCw, Search, SearchX, UnlockKeyhole } from "lucide-react";

import type { Space } from "../api/types";
import type { SpaceUsage } from "../api/usage";
import { formatBytes } from "../shared/lib/formatBytes";
import { useUiStore } from "../stores/uiStore";

const SAVE_LABEL: Record<string, { text: string; dot: string }> = {
  idle: { text: "ready", dot: "bg-success" },
  saving: { text: "saving…", dot: "bg-warning" },
  saved: { text: "saved", dot: "bg-success" },
  error: { text: "save failed", dot: "bg-danger" },
  conflict: { text: "conflict", dot: "bg-warning" }
};

export function StatusBar({
  activeSpace,
  usage
}: {
  activeSpace: Space | null;
  usage?: SpaceUsage;
}) {
  const saveState = useUiStore((state) => state.saveState);
  const status = SAVE_LABEL[saveState] ?? SAVE_LABEL.idle;
  const usedBytes = usage ? usage.text_bytes.used + usage.file_bytes.used : 0;
  const itemUsageLabel = usage
    ? `${usage.items.used.toLocaleString()} of ${usage.items.limit.toLocaleString()} items`
    : "";
  const storageUsageLabel = usage
    ? `Text ${formatBytes(usage.text_bytes.used)} of ${formatBytes(usage.text_bytes.limit)}; Files ${formatBytes(usage.file_bytes.used)} of ${formatBytes(usage.file_bytes.limit)}`
    : "";

  return (
    <footer className="hidden h-7 items-center justify-between gap-4 border-t border-seam bg-surface px-3 text-xs text-muted md:flex">
      <span className="flex shrink-0 items-center gap-2"><span className={`size-2 rounded-full ${status.dot}`} aria-hidden="true" /> {status.text}</span>
      <div className="flex min-w-0 items-center gap-4">
        {usage ? (
          <span className="flex shrink-0 items-center gap-3 text-faint">
            <span className="flex items-center gap-1" title={itemUsageLabel}>
              <ListTree size={13} aria-hidden="true" />
              {usage.items.used.toLocaleString()} items
            </span>
            <span className="flex items-center gap-1" title={storageUsageLabel}>
              <HardDrive size={13} aria-hidden="true" />
              {formatBytes(usedBytes)} used
            </span>
            {usage.reconciliation_pending ? (
              <span role="status" title="Usage is updating" aria-label="Usage is updating">
                <RefreshCw size={12} aria-hidden="true" />
              </span>
            ) : null}
          </span>
        ) : null}
        <span className="flex min-w-0 items-center gap-2">
          {activeSpace ? (
            <>
              <span
                role="img"
                title={`New items ${activeSpace.default_search_enabled ? "are" : "are not"} included in search`}
                aria-label={`New items ${activeSpace.default_search_enabled ? "are" : "are not"} included in search`}
              >
                {activeSpace.default_search_enabled ? <Search size={13} aria-hidden="true" /> : <SearchX size={13} aria-hidden="true" />}
              </span>
              <span
                role="img"
                title={`New document encryption is ${activeSpace.default_text_encryption_enabled ? "on" : "off"}`}
                aria-label={`New document encryption is ${activeSpace.default_text_encryption_enabled ? "on" : "off"}`}
              >
                {activeSpace.default_text_encryption_enabled ? <LockKeyhole size={13} aria-hidden="true" /> : <UnlockKeyhole size={13} aria-hidden="true" />}
              </span>
            </>
          ) : null}
          <span className="max-w-48 truncate">{activeSpace?.name ?? "No space"}</span>
        </span>
      </div>
    </footer>
  );
}
