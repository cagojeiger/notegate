import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import {
  abortFileUpload,
  beginFileUpload,
  completeFileUpload,
  transferFile,
  type FileUploadInput
} from "../../api/files";
import { invalidateNodeLists } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";

export type UploadTaskStatus = "queued" | "preparing" | "uploading" | "finalizing" | "failed" | "completed";

export type UploadTask = FileUploadInput & {
  id: string;
  spaceId: string;
  spaceName: string;
  destinationPath: string;
  status: UploadTaskStatus;
  uploadedBytes: number;
  error: string | null;
};

export type StartUploadInput = FileUploadInput & {
  spaceId: string;
  spaceName: string;
  destinationPath: string;
};

type UploadState = {
  tasks: UploadTask[];
  activeCount: number;
  queuedCount: number;
  failedCount: number;
};

type UploadActions = {
  startUpload: (input: StartUploadInput) => string;
  cancelUpload: (taskId: string) => void;
  retryUpload: (taskId: string) => void;
  dismissUpload: (taskId: string) => void;
};

type UploadManager = UploadState & UploadActions;

type UploadRun = {
  taskId: string;
  input: StartUploadInput;
  controller: AbortController;
};

const COMPLETED_TASK_TTL_MS = 4_000;
const MAX_CONCURRENT_UPLOADS = 2;
const UploadStateContext = createContext<UploadState | null>(null);
const UploadActionsContext = createContext<UploadActions | null>(null);
let nextTaskId = 0;

export function UploadProvider({ children }: { children: ReactNode }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const [tasks, setTasks] = useState<UploadTask[]>([]);
  const tasksRef = useRef(tasks);
  const controllers = useRef(new Map<string, AbortController>());
  const cleanupTimers = useRef(new Map<string, number>());
  tasksRef.current = tasks;

  const updateTask = useCallback((taskId: string, update: (task: UploadTask) => UploadTask) => {
    setTasks((current) => current.map((task) => task.id === taskId ? update(task) : task));
  }, []);

  const { mutateAsync: executeUpload } = useMutation({
    mutationFn: async ({ taskId, input, controller }: UploadRun) => {
      updateTask(taskId, (task) => ({ ...task, status: "preparing", uploadedBytes: 0, error: null }));
      const upload = await beginFileUpload(client, input.spaceId, input);
      try {
        if (controller.signal.aborted) throw canceledError();

        updateTask(taskId, (task) => ({ ...task, status: "uploading" }));
        const completedParts = await transferFile(client, input.spaceId, upload, input.file, {
          signal: controller.signal,
          onProgress: (uploadedBytes) => updateTask(taskId, (task) => ({ ...task, uploadedBytes }))
        });
        if (controller.signal.aborted) throw canceledError();

        updateTask(taskId, (task) => ({ ...task, status: "finalizing", uploadedBytes: input.file.size }));
        return await completeFileUpload(client, input.spaceId, upload.upload_id, completedParts);
      } catch (error) {
        // Cleanup runs in the background so the task can settle immediately.
        // Abandoned uploads also expire server-side.
        void abortFileUpload(client, input.spaceId, upload.upload_id).catch(() => undefined);
        throw error;
      }
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
      .then((response) => {
        controllers.current.delete(taskId);
        updateTask(taskId, (task) => ({ ...task, status: "completed", uploadedBytes: task.file.size }));
        queryClient.setQueryData(queryKeys.node(input.spaceId, response.node.id), response.node);
        invalidateNodeLists(queryClient, input.spaceId, [input.parentNodeId]);
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
      });
  }, [executeUpload, queryClient, removeTask, updateTask]);

  const startUpload = useCallback((input: StartUploadInput) => {
    const taskId = `upload-${Date.now()}-${nextTaskId++}`;
    setTasks((current) => [{
      ...input,
      id: taskId,
      status: "queued",
      uploadedBytes: 0,
      error: null
    }, ...current]);
    return taskId;
  }, []);

  const cancelUpload = useCallback((taskId: string) => {
    const controller = controllers.current.get(taskId);
    if (controller) controller.abort();
    else removeTask(taskId);
  }, [removeTask]);

  const retryUpload = useCallback((taskId: string) => {
    const task = tasksRef.current.find((candidate) => candidate.id === taskId);
    if (!task || task.status !== "failed") return;
    const queuedTask: UploadTask = { ...task, status: "queued", uploadedBytes: 0, error: null };
    tasksRef.current = tasksRef.current.map((candidate) => candidate.id === taskId ? queuedTask : candidate);
    setTasks((current) => current.map((candidate) => candidate.id === taskId ? queuedTask : candidate));
  }, []);

  const dismissUpload = useCallback((taskId: string) => {
    const task = tasksRef.current.find((candidate) => candidate.id === taskId);
    if (!task || isActive(task.status) || controllers.current.has(taskId)) return;
    removeTask(taskId);
  }, [removeTask]);

  useEffect(() => () => {
    for (const controller of controllers.current.values()) controller.abort();
    for (const timer of cleanupTimers.current.values()) window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    const available = MAX_CONCURRENT_UPLOADS - controllers.current.size;
    if (available <= 0) return;
    const queued = tasks
      .filter((task) => task.status === "queued")
      .slice(-available)
      .reverse();
    for (const task of queued) runUpload(task.id, task);
  }, [runUpload, tasks]);

  const state = useMemo<UploadState>(() => ({ tasks, ...summarizeUploads(tasks) }), [tasks]);
  const actions = useMemo<UploadActions>(() => ({
    startUpload,
    cancelUpload,
    retryUpload,
    dismissUpload
  }), [cancelUpload, dismissUpload, retryUpload, startUpload]);

  return (
    <UploadActionsContext.Provider value={actions}>
      <UploadStateContext.Provider value={state}>{children}</UploadStateContext.Provider>
    </UploadActionsContext.Provider>
  );
}

export function useUploadManager(): UploadManager {
  const state = useContext(UploadStateContext);
  const actions = useUploadActions();
  if (!state) throw new Error("UploadProvider is missing");
  return { ...state, ...actions };
}

export function useUploadActions(): UploadActions {
  const actions = useContext(UploadActionsContext);
  if (!actions) throw new Error("UploadProvider is missing");
  return actions;
}

function summarizeUploads(tasks: UploadTask[]) {
  return {
    activeCount: tasks.filter((task) => isTransferring(task.status)).length,
    queuedCount: tasks.filter((task) => task.status === "queued").length,
    failedCount: tasks.filter((task) => task.status === "failed").length
  };
}

function isActive(status: UploadTaskStatus): boolean {
  return status === "queued" || isTransferring(status);
}

function isTransferring(status: UploadTaskStatus): boolean {
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
