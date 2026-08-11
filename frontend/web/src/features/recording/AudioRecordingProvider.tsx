import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState
} from "react";

import { useUiStore } from "../../stores/uiStore";
import { useUploadActions, useUploadManager } from "../uploads/UploadProvider";
import {
  AudioRecordingContextProvider,
  IDLE_RECORDING_STATE,
  type RecordingActions,
  type RecordingDestination,
  type RecordingState
} from "./AudioRecordingContext";
import type {
  AudioRecordingRuntime,
  CapturedRecording,
  RecordingRuntimeCallbacks
} from "./AudioRecordingRuntime";

type WakeLockSentinelLike = {
  release: () => Promise<void>;
};

type WakeLockNavigator = Navigator & {
  wakeLock?: {
    request: (type: "screen") => Promise<WakeLockSentinelLike>;
  };
};

const EMPTY_SIGNAL = Array.from({ length: 12 }, () => 0);

export function AudioRecordingProvider({ children }: { children: ReactNode }) {
  const { startUpload } = useUploadActions();
  const { tasks } = useUploadManager();
  const showToast = useUiStore((store) => store.showToast);
  const [state, setState] = useState<RecordingState>(IDLE_RECORDING_STATE);
  const [signal, setSignal] = useState<number[]>(EMPTY_SIGNAL);
  const stateRef = useRef(state);
  const runtimeRef = useRef<AudioRecordingRuntime | null>(null);
  const startRequestRef = useRef(0);
  const mountedRef = useRef(true);
  const recordingUploadIdsRef = useRef(new Set<string>());
  const seenUploadIdsRef = useRef(new Set<string>());
  const recordingUploadActiveRef = useRef(false);
  const wakeLockRef = useRef<WakeLockSentinelLike | null>(null);
  stateRef.current = state;

  const releaseWakeLock = useCallback(() => {
    const sentinel = wakeLockRef.current;
    wakeLockRef.current = null;
    if (sentinel) void sentinel.release().catch(() => undefined);
  }, []);

  const acquireWakeLock = useCallback(async () => {
    if (document.visibilityState !== "visible") return false;
    if (wakeLockRef.current) return true;
    const wakeLock = (navigator as WakeLockNavigator).wakeLock;
    if (!wakeLock) return false;
    try {
      wakeLockRef.current = await wakeLock.request("screen");
      return true;
    } catch {
      return false;
    }
  }, []);

  const queueCapturedRecording = useCallback((capture: CapturedRecording) => {
    const taskId = startUpload({
      ...capture.destination,
      name: capture.file.name,
      file: capture.file,
      nodeMetadata: capture.nodeMetadata
    });
    recordingUploadIdsRef.current.add(taskId);
    recordingUploadActiveRef.current = true;
    showToast("Recording queued for upload");
  }, [showToast, startUpload]);

  const startRecording = useCallback(async (destination: RecordingDestination) => {
    if (stateRef.current.status !== "idle") return;
    const requestId = startRequestRef.current + 1;
    startRequestRef.current = requestId;
    setState({
      status: "requesting",
      startedAt: null,
      activeSegmentStartedAt: null,
      activePauseStartedAt: null,
      recordedDurationMs: 0,
      pausedDurationMs: 0,
      segmentCount: 0,
      filename: null,
      destinationPath: destination.destinationPath
    });

    try {
      const { AudioRecordingRuntime: Runtime } = await import("./AudioRecordingRuntime");
      if (!mountedRef.current || requestId !== startRequestRef.current) return;
      const runtime = runtimeRef.current ?? new Runtime();
      runtimeRef.current = runtime;
      const callbacks: RecordingRuntimeCallbacks = {
        acquireWakeLock,
        onCaptured: queueCapturedRecording,
        onSignal: setSignal,
        onState: setState,
        onToast: showToast
      };
      await runtime.start(destination, callbacks);
    } catch {
      if (!mountedRef.current || requestId !== startRequestRef.current) return;
      setState(IDLE_RECORDING_STATE);
      showToast("Could not start audio recording");
    }
  }, [acquireWakeLock, queueCapturedRecording, showToast]);

  const stopRecording = useCallback(() => {
    runtimeRef.current?.stop();
  }, []);

  const pauseRecording = useCallback(() => {
    runtimeRef.current?.pause();
  }, []);

  const resumeRecording = useCallback(() => {
    runtimeRef.current?.resume();
  }, []);

  const discardRecording = useCallback(() => {
    runtimeRef.current?.discard();
  }, []);

  useEffect(() => {
    let uploadNeedsWakeLock = false;
    for (const taskId of recordingUploadIdsRef.current) {
      const task = tasks.find((candidate) => candidate.id === taskId);
      if (!task) {
        if (seenUploadIdsRef.current.has(taskId)) {
          recordingUploadIdsRef.current.delete(taskId);
          seenUploadIdsRef.current.delete(taskId);
        } else {
          uploadNeedsWakeLock = true;
        }
        continue;
      }

      seenUploadIdsRef.current.add(taskId);
      if (task.status === "completed") {
        recordingUploadIdsRef.current.delete(taskId);
        seenUploadIdsRef.current.delete(taskId);
        showToast("Recording saved");
      } else if (task.status !== "failed") {
        uploadNeedsWakeLock = true;
      }
    }

    recordingUploadActiveRef.current = uploadNeedsWakeLock;
    const captureNeedsWakeLock = state.status === "recording"
      || state.status === "paused"
      || state.status === "stopping";
    if (captureNeedsWakeLock || uploadNeedsWakeLock) {
      void acquireWakeLock();
    } else {
      releaseWakeLock();
    }
  }, [acquireWakeLock, releaseWakeLock, showToast, state.status, tasks]);

  useEffect(() => {
    function handleVisibilityChange() {
      if (
        document.visibilityState === "visible"
        && (
          stateRef.current.status === "recording"
          || stateRef.current.status === "paused"
          || stateRef.current.status === "stopping"
          || recordingUploadActiveRef.current
        )
      ) {
        void acquireWakeLock();
      }
    }
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      if (stateRef.current.status === "idle" && !recordingUploadActiveRef.current) return;
      event.preventDefault();
    }
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("beforeunload", handleBeforeUnload);
    };
  }, [acquireWakeLock]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      startRequestRef.current += 1;
      runtimeRef.current?.dispose();
      releaseWakeLock();
    };
  }, [releaseWakeLock]);

  const actions = useMemo<RecordingActions>(() => ({
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    discardRecording
  }), [discardRecording, pauseRecording, resumeRecording, startRecording, stopRecording]);

  return <AudioRecordingContextProvider actions={actions} signal={signal} state={state}>{children}</AudioRecordingContextProvider>;
}

export {
  useAudioRecordingActions,
  useAudioRecordingSignal,
  useAudioRecordingState
} from "./AudioRecordingContext";
export type {
  RecordingActions,
  RecordingDestination,
  RecordingState,
  RecordingStatus
} from "./AudioRecordingContext";
