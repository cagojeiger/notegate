import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { beginFileUpload, completeFileUpload, transferFile, type FileUploadInput } from "../../api/files";
import { queryKeys } from "../../api/queryKeys";
import { useUiStore } from "../../stores/uiStore";

export type UploadTaskStatus = "preparing" | "uploading" | "finalizing" | "failed" | "completed";

export type UploadTask = FileUploadInput & {
  id: string;
  spaceId: string;
  spaceName: string;
  status: UploadTaskStatus;
  uploadedBytes: number;
  error: string | null;
};

export type StartUploadInput = FileUploadInput & {
  spaceId: string;
  spaceName: string;
};

type UploadManager = {
  tasks: UploadTask[];
  activeCount: number;
  failedCount: number;
  progressPercent: number;
  startUpload: (input: StartUploadInput) => string;
  cancelUpload: (taskId: string) => void;
  retryUpload: (taskId: string) => void;
  dismissUpload: (taskId: string) => void;
};

type UploadRun = {
  taskId: string;
  input: StartUploadInput;
  controller: AbortController;
};

const COMPLETED_TASK_TTL_MS = 4_000;
const UploadContext = createContext<UploadManager | null>(null);
let nextTaskId = 0;

export function UploadProvider({ children }: { children: ReactNode }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const [tasks, setTasks] = useState<UploadTask[]>([]);
  const controllers = useRef(new Map<string, AbortController>());
  const cleanupTimers = useRef(new Map<string, number>());

  const updateTask = useCallback((taskId: string, update: (task: UploadTask) => UploadTask) => {
    setTasks((current) => current.map((task) => task.id === taskId ? update(task) : task));
  }, []);

  const { mutateAsync: executeUpload } = useMutation({
    mutationFn: async ({ taskId, input, controller }: UploadRun) => {
      updateTask(taskId, (task) => ({ ...task, status: "preparing", uploadedBytes: 0, error: null }));
      const upload = await beginFileUpload(client, input.spaceId, input);
      if (controller.signal.aborted) throw canceledError();

      updateTask(taskId, (task) => ({ ...task, status: "uploading" }));
      await transferFile(upload, input.file, {
        signal: controller.signal,
        onProgress: (uploadedBytes) => updateTask(taskId, (task) => ({ ...task, uploadedBytes }))
      });
      if (controller.signal.aborted) throw canceledError();

      updateTask(taskId, (task) => ({ ...task, status: "finalizing", uploadedBytes: input.file.size }));
      return completeFileUpload(client, input.spaceId, upload.upload_id);
    },
    meta: { silentError: true }
  });

  const removeTask = useCallback((taskId: string) => {
    const timer = cleanupTimers.current.get(taskId);
    if (timer !== undefined) window.clearTimeout(timer);
    cleanupTimers.current.delete(taskId);
    controllers.current.delete(taskId);
    setTasks((current) => current.filter((task) => task.id !== taskId));
  }, []);

  const runUpload = useCallback((taskId: string, input: StartUploadInput) => {
    if (controllers.current.has(taskId)) return;
    const controller = new AbortController();
    controllers.current.set(taskId, controller);
    void executeUpload({ taskId, input, controller })
      .then(() => {
        controllers.current.delete(taskId);
        updateTask(taskId, (task) => ({ ...task, status: "completed", uploadedBytes: task.file.size }));
        void queryClient.invalidateQueries({ queryKey: queryKeys.spaces, exact: true });
        void queryClient.invalidateQueries({ queryKey: ["spaces", input.spaceId] });
        useUiStore.getState().showToast(`Uploaded ${input.name}`);
        const timer = window.setTimeout(() => removeTask(taskId), COMPLETED_TASK_TTL_MS);
        cleanupTimers.current.set(taskId, timer);
      })
      .catch((error: unknown) => {
        controllers.current.delete(taskId);
        if (controller.signal.aborted || isCanceled(error)) {
          removeTask(taskId);
          return;
        }
        updateTask(taskId, (task) => ({ ...task, status: "failed", error: uploadErrorMessage(error) }));
        useUiStore.getState().showToast(`Upload failed: ${input.name}`);
      });
  }, [executeUpload, queryClient, removeTask, updateTask]);

  const startUpload = useCallback((input: StartUploadInput) => {
    const taskId = `upload-${Date.now()}-${nextTaskId++}`;
    setTasks((current) => [{
      ...input,
      id: taskId,
      status: "preparing",
      uploadedBytes: 0,
      error: null
    }, ...current]);
    runUpload(taskId, input);
    return taskId;
  }, [runUpload]);

  const cancelUpload = useCallback((taskId: string) => {
    controllers.current.get(taskId)?.abort();
  }, []);

  const retryUpload = useCallback((taskId: string) => {
    const task = tasks.find((candidate) => candidate.id === taskId);
    if (!task || task.status !== "failed") return;
    runUpload(taskId, task);
  }, [runUpload, tasks]);

  const dismissUpload = useCallback((taskId: string) => {
    const task = tasks.find((candidate) => candidate.id === taskId);
    if (!task || isActive(task.status)) return;
    removeTask(taskId);
  }, [removeTask, tasks]);

  useEffect(() => () => {
    for (const controller of controllers.current.values()) controller.abort();
    for (const timer of cleanupTimers.current.values()) window.clearTimeout(timer);
  }, []);

  const summary = useMemo(() => summarizeUploads(tasks), [tasks]);
  const value = useMemo<UploadManager>(() => ({
    tasks,
    ...summary,
    startUpload,
    cancelUpload,
    retryUpload,
    dismissUpload
  }), [cancelUpload, dismissUpload, retryUpload, startUpload, summary, tasks]);

  return <UploadContext.Provider value={value}>{children}</UploadContext.Provider>;
}

export function useUploadManager(): UploadManager {
  const manager = useContext(UploadContext);
  if (!manager) throw new Error("UploadProvider is missing");
  return manager;
}

function summarizeUploads(tasks: UploadTask[]) {
  const active = tasks.filter((task) => isActive(task.status));
  const totalBytes = active.reduce((sum, task) => sum + task.file.size, 0);
  const uploadedBytes = active.reduce((sum, task) => sum + task.uploadedBytes, 0);
  return {
    activeCount: active.length,
    failedCount: tasks.filter((task) => task.status === "failed").length,
    progressPercent: totalBytes > 0 ? Math.min(100, Math.round((uploadedBytes / totalBytes) * 100)) : 0
  };
}

function isActive(status: UploadTaskStatus): boolean {
  return status === "preparing" || status === "uploading" || status === "finalizing";
}

function canceledError(): DOMException {
  return new DOMException("File upload canceled", "AbortError");
}

function isCanceled(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

function uploadErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "File upload failed";
}
