import { Check, RotateCcw, Trash2, X } from "lucide-react";
import type { ReactNode } from "react";

import { formatBytes } from "../../shared/lib/formatBytes";
import { EmptyState } from "../../shared/ui";
import { type UploadTask, useUploadManager } from "./UploadProvider";

const STATUS_LABELS: Record<UploadTask["status"], string> = {
  preparing: "Preparing",
  uploading: "Uploading",
  finalizing: "Finalizing",
  failed: "Failed",
  completed: "Complete"
};

export function UploadTransfersPanel() {
  const { tasks, cancelUpload, retryUpload, dismissUpload } = useUploadManager();

  if (tasks.length === 0) return <EmptyState>No active transfers.</EmptyState>;

  return (
    <section aria-label="File transfers">
      <ol className="rounded-lg border border-border bg-surface px-4">
        {tasks.map((task) => (
          <UploadTransferRow
            key={task.id}
            task={task}
            onCancel={() => cancelUpload(task.id)}
            onRetry={() => retryUpload(task.id)}
            onDismiss={() => dismissUpload(task.id)}
          />
        ))}
      </ol>
    </section>
  );
}

function UploadTransferRow({
  task,
  onCancel,
  onRetry,
  onDismiss
}: {
  task: UploadTask;
  onCancel: () => void;
  onRetry: () => void;
  onDismiss: () => void;
}) {
  const progress = task.file.size > 0 ? Math.min(100, Math.round((task.uploadedBytes / task.file.size) * 100)) : 0;
  const status = task.status === "uploading" ? `${STATUS_LABELS[task.status]} ${progress}%` : STATUS_LABELS[task.status];

  return (
    <li className="border-b border-seam py-3 last:border-b-0">
      <div className="flex min-w-0 items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium text-text" title={task.name}>{task.name}</div>
          <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted">
            <span className="truncate">{task.spaceName}</span>
            <span className="shrink-0" aria-hidden="true">·</span>
            <span className="shrink-0">{formatBytes(task.file.size)}</span>
            {task.error ? <><span className="shrink-0" aria-hidden="true">·</span><span className="truncate text-danger" title={task.error}>{task.error}</span></> : null}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <span className={`text-xs ${task.status === "failed" ? "text-danger" : "text-muted"}`}>{status}</span>
          {task.status === "preparing" || task.status === "uploading" ? <IconButton label={`Cancel upload ${task.name}`} onClick={onCancel}><X size={14} /></IconButton> : null}
          {task.status === "failed" ? <IconButton label={`Retry upload ${task.name}`} onClick={onRetry}><RotateCcw size={14} /></IconButton> : null}
          {task.status === "failed" ? <IconButton label={`Dismiss upload ${task.name}`} onClick={onDismiss}><Trash2 size={14} /></IconButton> : null}
          {task.status === "completed" ? <Check size={14} className="text-success" aria-hidden="true" /> : null}
        </div>
      </div>
      {task.status !== "failed" && task.status !== "completed" ? (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-seam" aria-label={`${task.name} upload progress`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress}>
          <div className="h-full bg-primary transition-[width]" style={{ width: `${progress}%` }} />
        </div>
      ) : null}
    </li>
  );
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" aria-label={label} title={label} onClick={onClick} className="grid size-7 place-items-center rounded-lg text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45">
      {children}
    </button>
  );
}
