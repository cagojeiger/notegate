import { Check, ChevronDown, RotateCcw, UploadCloud, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { formatBytes } from "../../shared/lib/formatBytes";
import { useUploadManager, type UploadTask } from "./UploadProvider";

export function UploadProgressDock() {
  const { tasks, activeCount, failedCount, cancelUpload, retryUpload, dismissUpload } = useUploadManager();
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    if (tasks.length === 0) setCollapsed(false);
  }, [tasks.length]);

  if (tasks.length === 0) return null;

  return (
    <section
      aria-label="File uploads"
      className="z-20 shrink-0 border-t border-seam bg-surface text-text md:fixed md:bottom-10 md:right-3 md:w-96 md:overflow-hidden md:rounded-lg md:border md:border-border md:shadow-[var(--ng-focus-shadow)]"
    >
      <button
        type="button"
        onClick={() => setCollapsed((value) => !value)}
        className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left outline-none hover:bg-[var(--ng-hover)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
        aria-label={`${collapsed ? "Expand" : "Collapse"} uploads`}
        aria-expanded={!collapsed}
        aria-controls="upload-progress-list"
      >
        <span className="flex min-w-0 items-center gap-2 text-sm font-medium">
          <UploadCloud size={15} className="shrink-0 text-primary" aria-hidden="true" />
          <span>Uploads</span>
          <span className="truncate text-xs font-normal text-muted">{uploadSummary(tasks.length, activeCount, failedCount)}</span>
        </span>
        <ChevronDown size={15} className={`shrink-0 text-muted transition ${collapsed ? "-rotate-90" : ""}`} aria-hidden="true" />
      </button>

      {!collapsed ? (
        <ol id="upload-progress-list" className="max-h-56 overflow-y-auto border-t border-seam md:max-h-[40vh]">
          {tasks.map((task) => (
            <UploadProgressRow
              key={task.id}
              task={task}
              onCancel={() => cancelUpload(task.id)}
              onRetry={() => retryUpload(task.id)}
              onDismiss={() => dismissUpload(task.id)}
            />
          ))}
        </ol>
      ) : null}
    </section>
  );
}

function UploadProgressRow({
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
  const destination = `${task.spaceName}${task.destinationPath === "/" ? "" : task.destinationPath}`;
  const showProgress = task.status === "preparing" || task.status === "uploading" || task.status === "finalizing";

  return (
    <li className="border-b border-seam px-3 py-2.5 last:border-b-0">
      <div className="flex min-w-0 items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-medium text-text" title={task.name}>{task.name}</div>
          <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted">
            <span className="truncate" title={destination}>{destination}</span>
            <span className="shrink-0" aria-hidden="true">·</span>
            <span className="shrink-0">{formatBytes(task.file.size)}</span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <UploadStatus task={task} progress={progress} />
          {isCancelable(task.status) ? <IconButton label={`Cancel upload ${task.name}`} onClick={onCancel}><X size={14} /></IconButton> : null}
          {task.status === "failed" ? <IconButton label={`Retry upload ${task.name}`} onClick={onRetry}><RotateCcw size={14} /></IconButton> : null}
          {task.status === "failed" ? <IconButton label={`Dismiss upload ${task.name}`} onClick={onDismiss}><X size={14} /></IconButton> : null}
          {task.status === "completed" ? <Check size={14} className="text-success" aria-hidden="true" /> : null}
        </div>
      </div>
      {showProgress ? (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-seam" aria-label={`${task.name} upload progress`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress}>
          <div className={`h-full transition-[width] ${task.status === "finalizing" ? "bg-success" : "bg-primary"}`} style={{ width: `${progress}%` }} />
        </div>
      ) : null}
      {task.status === "failed" && task.error ? (
        <div className="mt-1 truncate text-xs text-danger" title={task.error}>{task.error}</div>
      ) : null}
    </li>
  );
}

function UploadStatus({ task, progress }: { task: UploadTask; progress: number }) {
  if (task.status === "uploading") return <span className="text-xs tabular-nums text-muted">{progress}%</span>;
  if (task.status === "preparing") return <span className="text-xs text-muted">Preparing</span>;
  if (task.status === "finalizing") return <span className="text-xs text-muted">Finalizing</span>;
  if (task.status === "failed") return <span className="text-xs text-danger">Failed</span>;
  return <span className="text-xs text-muted">Complete</span>;
}

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" aria-label={label} title={label} onClick={onClick} className="grid size-7 place-items-center rounded-md text-muted outline-none hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45">
      {children}
    </button>
  );
}

function uploadSummary(taskCount: number, activeCount: number, failedCount: number): string {
  if (activeCount > 0) return `${activeCount} active${failedCount > 0 ? ` · ${failedCount} failed` : ""}`;
  if (failedCount > 0) return `${failedCount} failed`;
  return `${taskCount} complete`;
}

function isCancelable(status: UploadTask["status"]): boolean {
  return status === "preparing" || status === "uploading";
}
