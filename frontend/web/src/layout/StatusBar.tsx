import { UploadCloud } from "lucide-react";

import type { Space } from "../api/types";
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
  activeUploads,
  failedUploads,
  uploadProgress,
  onOpenTransfers
}: {
  activeSpace: Space | null;
  activeUploads: number;
  failedUploads: number;
  uploadProgress: number;
  onOpenTransfers: () => void;
}) {
  const saveState = useUiStore((state) => state.saveState);
  const status = SAVE_LABEL[saveState] ?? SAVE_LABEL.idle;
  const transferLabel = [
    activeUploads > 0 ? `${activeUploads} uploading · ${uploadProgress}%` : null,
    failedUploads > 0 ? `${failedUploads} failed` : null
  ].filter(Boolean).join(" · ");
  return (
    <footer className="hidden h-7 items-center justify-between border-t border-seam bg-surface px-3 text-xs text-muted md:flex">
      <span className="flex items-center gap-2"><span className={`size-2 rounded-full ${status.dot}`} /> {status.text}</span>
      <span className="flex items-center gap-4">
        {transferLabel ? (
          <button type="button" onClick={onOpenTransfers} className={`flex items-center gap-1.5 hover:text-text ${failedUploads > 0 ? "text-danger" : ""}`} aria-label="Open file transfers">
            <UploadCloud size={13} /> {transferLabel}
          </button>
        ) : null}
        <span>{activeSpace?.name ?? "No space"}</span>
      </span>
    </footer>
  );
}
