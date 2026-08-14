import { Check, ChevronDown, RotateCcw, UploadCloud, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { formatBytes } from "../../shared/lib/formatBytes";
import { IconButton } from "../../shared/ui";
import { useUploadManager, type UploadTask } from "./UploadProvider";

const UPLOAD_STATUS_COPY = {
  queued: { visible: "Queued", announcement: "Queued" },
  preparing: { visible: "Preparing", announcement: "Preparing upload" },
  uploading: { visible: "Uploading", announcement: "Uploading" },
  finalizing: { visible: "Finalizing", announcement: "Finalizing upload" },
  failed: { visible: "Failed", announcement: "Upload failed" },
  completed: { visible: "Complete", announcement: "Upload complete" }
} satisfies Record<UploadTask["status"], { visible: string; announcement: string }>;

export function UploadProgressDock() {
  const manager = useUploadManager();
  const announcement = useUploadAnnouncement(manager.tasks);

  return (
    <>
      <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {announcement.message ? <span key={announcement.sequence}>{announcement.message}</span> : null}
      </p>
      {manager.tasks.length > 0 ? <UploadProgressPanel manager={manager} /> : null}
    </>
  );
}

function UploadProgressPanel({ manager }: { manager: ReturnType<typeof useUploadManager> }) {
  const { tasks, activeCount, queuedCount, failedCount, cancelUpload, retryUpload, dismissUpload } = manager;
  const [collapsed, setCollapsed] = useState(false);

  return (
    <section
      aria-label="File uploads"
      className="pointer-events-auto shrink-0 border-t border-seam bg-surface text-text md:overflow-hidden md:rounded-lg md:border md:border-border md:shadow-[var(--ng-focus-shadow)]"
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
          <span className="truncate text-xs font-normal text-muted">{uploadSummary(tasks.length, activeCount, queuedCount, failedCount)}</span>
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
          {isCancelable(task.status) ? <IconButton label={`Cancel upload ${task.name}`} size="sm" onClick={onCancel}><X size={14} /></IconButton> : null}
          {task.status === "failed" ? <IconButton label={`Retry upload ${task.name}`} size="sm" onClick={onRetry}><RotateCcw size={14} /></IconButton> : null}
          {task.status === "failed" ? <IconButton label={`Dismiss upload ${task.name}`} size="sm" onClick={onDismiss}><X size={14} /></IconButton> : null}
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
  return (
    <span className={`text-xs ${task.status === "failed" ? "text-danger" : "text-muted"}`}>
      {UPLOAD_STATUS_COPY[task.status].visible}
    </span>
  );
}

function uploadSummary(taskCount: number, activeCount: number, queuedCount: number, failedCount: number): string {
  const pending = [
    activeCount > 0 ? `${activeCount} active` : null,
    queuedCount > 0 ? `${queuedCount} queued` : null,
    failedCount > 0 ? `${failedCount} failed` : null
  ].filter((value): value is string => value !== null);
  if (pending.length > 0) return pending.join(" · ");
  return `${taskCount} complete`;
}

function useUploadAnnouncement(tasks: UploadTask[]): { message: string; sequence: number } {
  const previousStatuses = useRef(new Map<string, UploadTask["status"]>());
  const [announcement, setAnnouncement] = useState({ message: "", sequence: 0 });

  useEffect(() => {
    const nextStatuses = new Map(tasks.map((task) => [task.id, task.status]));
    const changed = tasks.filter((task) => previousStatuses.current.get(task.id) !== task.status);
    const removed = [...previousStatuses.current.keys()].some((id) => !nextStatuses.has(id));
    previousStatuses.current = nextStatuses;
    if (tasks.length === 0) {
      setAnnouncement((current) => current.message
        ? { message: "", sequence: current.sequence + 1 }
        : current);
      return;
    }
    if (changed.length > 0) {
      const message = changed
        .map((task) => `${task.name}: ${UPLOAD_STATUS_COPY[task.status].announcement}`)
        .join(". ");
      setAnnouncement((current) => ({ message, sequence: current.sequence + 1 }));
    } else if (removed) {
      setAnnouncement((current) => current.message
        ? { message: "", sequence: current.sequence + 1 }
        : current);
    }
  }, [tasks]);

  return announcement;
}

function isCancelable(status: UploadTask["status"]): boolean {
  return status === "queued" || status === "preparing" || status === "uploading";
}
