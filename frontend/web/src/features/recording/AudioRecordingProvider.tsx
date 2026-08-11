import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState
} from "react";

import { useUiStore } from "../../stores/uiStore";
import { useUploadActions, useUploadManager } from "../uploads/UploadProvider";
import {
  MICROPHONE_CAPTURE_DEFAULTS,
  RECORDING_FORMAT,
  recordingFilename,
  recordingNodeMetadata,
  recordingSupport
} from "./audioRecording";

export type RecordingDestination = {
  spaceId: string;
  spaceName: string;
  parentNodeId: string;
  destinationPath: string;
};

export type RecordingStatus = "idle" | "requesting" | "recording" | "stopping";

type RecordingState = {
  status: RecordingStatus;
  startedAt: number | null;
  filename: string | null;
  destinationPath: string | null;
};

type RecordingActions = {
  startRecording: (destination: RecordingDestination) => Promise<void>;
  stopRecording: () => void;
  discardRecording: () => void;
};

type WakeLockSentinelLike = {
  release: () => Promise<void>;
};

type WakeLockNavigator = Navigator & {
  wakeLock?: {
    request: (type: "screen") => Promise<WakeLockSentinelLike>;
  };
};

const SIGNAL_BAR_COUNT = 12;
const SIGNAL_FRAME_INTERVAL_MS = 1_000 / 15;
const RECORDING_LOCK_NAME = "notegate:audio-recording";
const EMPTY_SIGNAL = Array.from({ length: SIGNAL_BAR_COUNT }, () => 0);
const IDLE_STATE: RecordingState = {
  status: "idle",
  startedAt: null,
  filename: null,
  destinationPath: null
};
const RecordingStateContext = createContext<RecordingState | null>(null);
const RecordingSignalContext = createContext<number[]>(EMPTY_SIGNAL);
const RecordingActionsContext = createContext<RecordingActions | null>(null);

export function AudioRecordingProvider({ children }: { children: ReactNode }) {
  const { startUpload } = useUploadActions();
  const { tasks } = useUploadManager();
  const showToast = useUiStore((store) => store.showToast);
  const [state, setState] = useState<RecordingState>(IDLE_STATE);
  const [signal, setSignal] = useState<number[]>(EMPTY_SIGNAL);
  const stateRef = useRef(state);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const signalFrameRef = useRef<number | null>(null);
  const lastSignalFrameRef = useRef(0);
  const chunksRef = useRef<Blob[]>([]);
  const destinationRef = useRef<RecordingDestination | null>(null);
  const discardRef = useRef(false);
  const recordingUploadIdsRef = useRef(new Set<string>());
  const seenUploadIdsRef = useRef(new Set<string>());
  const recordingUploadActiveRef = useRef(false);
  const recordingLockReleaseRef = useRef<(() => void) | null>(null);
  const wakeLockRef = useRef<WakeLockSentinelLike | null>(null);
  stateRef.current = state;

  const releaseWakeLock = useCallback(() => {
    const sentinel = wakeLockRef.current;
    wakeLockRef.current = null;
    if (sentinel) void sentinel.release().catch(() => undefined);
  }, []);

  const releaseRecordingLock = useCallback(() => {
    const release = recordingLockReleaseRef.current;
    recordingLockReleaseRef.current = null;
    release?.();
  }, []);

  const acquireRecordingLock = useCallback(async () => new Promise<boolean>((resolve) => {
    let resolved = false;
    void navigator.locks.request(RECORDING_LOCK_NAME, { ifAvailable: true }, async (lock) => {
      if (!lock) {
        resolved = true;
        resolve(false);
        return;
      }
      await new Promise<void>((release) => {
        recordingLockReleaseRef.current = release;
        resolved = true;
        resolve(true);
      });
    }).catch(() => {
      if (!resolved) resolve(false);
    });
  }), []);

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

  const stopSignal = useCallback(() => {
    if (signalFrameRef.current !== null) window.cancelAnimationFrame(signalFrameRef.current);
    signalFrameRef.current = null;
    lastSignalFrameRef.current = 0;
    const audioContext = audioContextRef.current;
    audioContextRef.current = null;
    if (audioContext) void audioContext.close().catch(() => undefined);
    setSignal(EMPTY_SIGNAL);
  }, []);

  const startSignal = useCallback((stream: MediaStream) => {
    try {
      const audioContext = new AudioContext();
      const source = audioContext.createMediaStreamSource(stream);
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 64;
      analyser.smoothingTimeConstant = 0.72;
      source.connect(analyser);
      audioContextRef.current = audioContext;
      void audioContext.resume().catch(() => undefined);
      const frequencyData = new Uint8Array(analyser.frequencyBinCount);

      function drawSignal(timestamp: number) {
        if (timestamp - lastSignalFrameRef.current >= SIGNAL_FRAME_INTERVAL_MS) {
          lastSignalFrameRef.current = timestamp;
          analyser.getByteFrequencyData(frequencyData);
          setSignal(sampleSignal(frequencyData));
        }
        signalFrameRef.current = window.requestAnimationFrame(drawSignal);
      }
      signalFrameRef.current = window.requestAnimationFrame(drawSignal);
    } catch {
      setSignal(EMPTY_SIGNAL);
    }
  }, []);

  const cleanupCapture = useCallback(() => {
    stopSignal();
    for (const track of streamRef.current?.getTracks() ?? []) track.stop();
    recorderRef.current = null;
    streamRef.current = null;
    chunksRef.current = [];
    destinationRef.current = null;
    releaseRecordingLock();
  }, [releaseRecordingLock, stopSignal]);

  const resetRecording = useCallback(() => {
    cleanupCapture();
    setState(IDLE_STATE);
  }, [cleanupCapture]);

  const startRecording = useCallback(async (destination: RecordingDestination) => {
    if (stateRef.current.status !== "idle") return;
    const support = recordingSupport();
    if (!support.supported) {
      showToast(support.reason ?? "Audio recording is unavailable");
      return;
    }

    setState({
      status: "requesting",
      startedAt: null,
      filename: null,
      destinationPath: destination.destinationPath
    });
    if (!await acquireRecordingLock()) {
      setState(IDLE_STATE);
      showToast("Audio is already recording in another NoteGate tab");
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: MICROPHONE_CAPTURE_DEFAULTS
      });
      const recorder = new MediaRecorder(stream, RECORDING_FORMAT);
      const audioTrack = stream.getAudioTracks()[0];
      const startedAt = Date.now();
      const mimeType = recorder.mimeType || RECORDING_FORMAT.mimeType;
      const filename = recordingFilename(new Date(startedAt), mimeType);

      streamRef.current = stream;
      recorderRef.current = recorder;
      chunksRef.current = [];
      destinationRef.current = destination;
      discardRef.current = false;

      recorder.addEventListener("dataavailable", (event) => {
        if (event.data.size > 0) chunksRef.current.push(event.data);
      });
      recorder.addEventListener("error", () => {
        discardRef.current = true;
        showToast("Recording stopped unexpectedly");
        resetRecording();
      });
      recorder.addEventListener("stop", () => {
        const shouldDiscard = discardRef.current;
        const recordingDestination = destinationRef.current;
        const recordedMimeType = recorder.mimeType || chunksRef.current[0]?.type || mimeType;
        const captureSettings = audioTrack?.getSettings() ?? {};
        const audioBitsPerSecond = recorder.audioBitsPerSecond;
        const file = new File(
          chunksRef.current,
          recordingFilename(new Date(startedAt), recordedMimeType),
          { type: recordedMimeType }
        );
        cleanupCapture();

        if (shouldDiscard || !recordingDestination) {
          setState(IDLE_STATE);
          return;
        }
        if (file.size === 0) {
          setState(IDLE_STATE);
          showToast("No audio was captured");
          return;
        }

        const taskId = startUpload({
          ...recordingDestination,
          name: file.name,
          file,
          nodeMetadata: recordingNodeMetadata(
            captureSettings,
            recordedMimeType,
            audioBitsPerSecond
          )
        });
        recordingUploadIdsRef.current.add(taskId);
        recordingUploadActiveRef.current = true;
        setState(IDLE_STATE);
        showToast("Recording queued for upload");
      }, { once: true });

      const awake = await acquireWakeLock();
      if (!awake) showToast("Keep NoteGate open until the recording upload finishes");
      startSignal(stream);
      recorder.start(5_000);
      setState({
        status: "recording",
        startedAt,
        filename,
        destinationPath: destination.destinationPath
      });
    } catch (error) {
      cleanupCapture();
      setState(IDLE_STATE);
      const denied = error instanceof DOMException
        && (error.name === "NotAllowedError" || error.name === "SecurityError");
      showToast(denied ? "Microphone permission is required" : "Could not start audio recording");
    }
  }, [acquireRecordingLock, acquireWakeLock, cleanupCapture, resetRecording, showToast, startSignal, startUpload]);

  const stopRecording = useCallback(() => {
    const recorder = recorderRef.current;
    if (!recorder || recorder.state === "inactive") return;
    discardRef.current = false;
    setState((current) => ({ ...current, status: "stopping" }));
    recorder.stop();
  }, []);

  const discardRecording = useCallback(() => {
    const recorder = recorderRef.current;
    if (!recorder || recorder.state === "inactive") return;
    discardRef.current = true;
    recorder.stop();
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

    if (recorderRef.current?.state === "recording" || uploadNeedsWakeLock) {
      void acquireWakeLock();
    } else {
      releaseWakeLock();
    }
  }, [acquireWakeLock, releaseWakeLock, showToast, state.status, tasks]);

  useEffect(() => {
    function handleVisibilityChange() {
      if (
        document.visibilityState === "visible"
        && (recorderRef.current?.state === "recording" || recordingUploadActiveRef.current)
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

  useEffect(() => () => {
    const recorder = recorderRef.current;
    if (recorder && recorder.state !== "inactive") {
      discardRef.current = true;
      recorder.stop();
    }
    cleanupCapture();
    releaseWakeLock();
  }, [cleanupCapture, releaseWakeLock]);

  const actions = useMemo<RecordingActions>(() => ({
    startRecording,
    stopRecording,
    discardRecording
  }), [discardRecording, startRecording, stopRecording]);

  return (
    <RecordingActionsContext.Provider value={actions}>
      <RecordingStateContext.Provider value={state}>
        <RecordingSignalContext.Provider value={signal}>{children}</RecordingSignalContext.Provider>
      </RecordingStateContext.Provider>
    </RecordingActionsContext.Provider>
  );
}

export function useAudioRecordingState(): RecordingState {
  const state = useContext(RecordingStateContext);
  if (!state) throw new Error("AudioRecordingProvider is missing");
  return state;
}

export function useAudioRecordingSignal(): number[] {
  return useContext(RecordingSignalContext);
}

export function useAudioRecordingActions(): RecordingActions {
  const actions = useContext(RecordingActionsContext);
  if (!actions) throw new Error("AudioRecordingProvider is missing");
  return actions;
}

function sampleSignal(frequencyData: Uint8Array): number[] {
  const usefulBins = Math.max(SIGNAL_BAR_COUNT, Math.min(24, frequencyData.length));
  return Array.from({ length: SIGNAL_BAR_COUNT }, (_, index) => {
    const start = Math.floor(index * usefulBins / SIGNAL_BAR_COUNT);
    const end = Math.max(start + 1, Math.floor((index + 1) * usefulBins / SIGNAL_BAR_COUNT));
    let peak = 0;
    for (let bin = start; bin < end; bin += 1) peak = Math.max(peak, frequencyData[bin] ?? 0);
    return Math.min(1, peak / 180);
  });
}
