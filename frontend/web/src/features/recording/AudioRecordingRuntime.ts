import type {
  RecordingDestination,
  RecordingState,
  RecordingStatus
} from "./AudioRecordingContext";
import {
  MICROPHONE_CAPTURE_DEFAULTS,
  RECORDING_FORMAT,
  recordingFilename,
  recordingNodeMetadata,
  recordingSupport
} from "./audioRecording";

export type CapturedRecording = {
  destination: RecordingDestination;
  file: File;
  nodeMetadata: Record<string, unknown>;
};

export type RecordingRuntimeCallbacks = {
  acquireWakeLock: () => Promise<boolean>;
  onCaptured: (capture: CapturedRecording) => void;
  onSignal: (signal: number[]) => void;
  onState: (state: RecordingState) => void;
  onToast: (message: string) => void;
};

const SIGNAL_BAR_COUNT = 12;
const SIGNAL_FRAME_INTERVAL_MS = 1_000 / 15;
const RECORDING_LOCK_NAME = "notegate:audio-recording";
const EMPTY_SIGNAL = Array.from({ length: SIGNAL_BAR_COUNT }, () => 0);

export class AudioRecordingRuntime {
  private status: RecordingStatus = "idle";
  private recorder: MediaRecorder | null = null;
  private stream: MediaStream | null = null;
  private audioContext: AudioContext | null = null;
  private signalFrame: number | null = null;
  private lastSignalFrame = 0;
  private chunks: Blob[] = [];
  private destination: RecordingDestination | null = null;
  private discardRequested = false;
  private releaseRecordingLock: (() => void) | null = null;
  private callbacks: RecordingRuntimeCallbacks | null = null;
  private disposed = false;

  async start(destination: RecordingDestination, callbacks: RecordingRuntimeCallbacks) {
    if (this.status !== "idle" || this.disposed) return;
    this.status = "requesting";
    this.callbacks = callbacks;
    const support = recordingSupport();
    if (!support.supported) {
      this.finishRequest(support.reason ?? "Audio recording is unavailable");
      return;
    }
    if (!await this.acquireRecordingLock()) {
      this.finishRequest("Audio is already recording in another NoteGate tab");
      return;
    }

    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: MICROPHONE_CAPTURE_DEFAULTS
      });
      if (this.disposed) {
        for (const track of stream.getTracks()) track.stop();
        this.cleanupCapture();
        return;
      }
      const recorder = new MediaRecorder(stream, RECORDING_FORMAT);
      const audioTrack = stream.getAudioTracks()[0];
      const startedAt = Date.now();
      const mimeType = recorder.mimeType || RECORDING_FORMAT.mimeType;
      const filename = recordingFilename(new Date(startedAt), mimeType);

      this.stream = stream;
      this.recorder = recorder;
      this.chunks = [];
      this.destination = destination;
      this.discardRequested = false;

      recorder.addEventListener("dataavailable", (event) => {
        if (event.data.size > 0) this.chunks.push(event.data);
      });
      recorder.addEventListener("error", () => {
        this.discardRequested = true;
        this.callbacks?.onToast("Recording stopped unexpectedly");
        this.cleanupCapture();
        this.setState("idle");
      });
      recorder.addEventListener("stop", () => {
        this.finishCapture(recorder, audioTrack, startedAt, mimeType);
      }, { once: true });

      const awake = await callbacks.acquireWakeLock();
      if (!awake) callbacks.onToast("Keep NoteGate open until the recording upload finishes");
      this.startSignal(stream);
      recorder.start(5_000);
      this.setState("recording", startedAt, filename, destination.destinationPath);
    } catch (error) {
      this.cleanupCapture();
      this.setState("idle");
      const denied = error instanceof DOMException
        && (error.name === "NotAllowedError" || error.name === "SecurityError");
      callbacks.onToast(denied ? "Microphone permission is required" : "Could not start audio recording");
    }
  }

  stop() {
    if (!this.recorder || this.recorder.state === "inactive") return;
    this.discardRequested = false;
    this.setState("stopping");
    this.recorder.stop();
  }

  discard() {
    if (!this.recorder || this.recorder.state === "inactive") return;
    this.discardRequested = true;
    this.recorder.stop();
  }

  dispose() {
    this.disposed = true;
    this.callbacks = null;
    if (this.recorder && this.recorder.state !== "inactive") {
      this.discardRequested = true;
      this.recorder.stop();
    }
    this.cleanupCapture();
    this.status = "idle";
  }

  private finishCapture(
    recorder: MediaRecorder,
    audioTrack: MediaStreamTrack | undefined,
    startedAt: number,
    fallbackMimeType: string
  ) {
    if (this.disposed) {
      this.cleanupCapture();
      return;
    }
    const callbacks = this.callbacks;
    const destination = this.destination;
    const shouldDiscard = this.discardRequested;
    const recordedMimeType = recorder.mimeType || this.chunks[0]?.type || fallbackMimeType;
    const captureSettings = audioTrack?.getSettings() ?? {};
    const file = new File(
      this.chunks,
      recordingFilename(new Date(startedAt), recordedMimeType),
      { type: recordedMimeType }
    );
    const nodeMetadata = recordingNodeMetadata(
      captureSettings,
      recordedMimeType,
      recorder.audioBitsPerSecond
    );
    this.cleanupCapture();
    this.setState("idle");

    if (!callbacks || shouldDiscard || !destination) return;
    if (file.size === 0) {
      callbacks.onToast("No audio was captured");
      return;
    }
    callbacks.onCaptured({ destination, file, nodeMetadata });
  }

  private finishRequest(message: string) {
    const callbacks = this.callbacks;
    this.cleanupCapture();
    this.setState("idle");
    callbacks?.onToast(message);
  }

  private setState(
    status: RecordingStatus,
    startedAt: number | null = null,
    filename: string | null = null,
    destinationPath: string | null = null
  ) {
    this.status = status;
    this.callbacks?.onState({ status, startedAt, filename, destinationPath });
  }

  private async acquireRecordingLock() {
    return new Promise<boolean>((resolve) => {
      let resolved = false;
      void navigator.locks.request(RECORDING_LOCK_NAME, { ifAvailable: true }, async (lock) => {
        if (!lock) {
          resolved = true;
          resolve(false);
          return;
        }
        await new Promise<void>((release) => {
          this.releaseRecordingLock = release;
          resolved = true;
          resolve(true);
        });
      }).catch(() => {
        if (!resolved) resolve(false);
      });
    });
  }

  private startSignal(stream: MediaStream) {
    try {
      const audioContext = new AudioContext();
      const source = audioContext.createMediaStreamSource(stream);
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 64;
      analyser.smoothingTimeConstant = 0.72;
      source.connect(analyser);
      this.audioContext = audioContext;
      void audioContext.resume().catch(() => undefined);
      const frequencyData = new Uint8Array(analyser.frequencyBinCount);

      const drawSignal = (timestamp: number) => {
        if (timestamp - this.lastSignalFrame >= SIGNAL_FRAME_INTERVAL_MS) {
          this.lastSignalFrame = timestamp;
          analyser.getByteFrequencyData(frequencyData);
          this.callbacks?.onSignal(sampleSignal(frequencyData));
        }
        this.signalFrame = window.requestAnimationFrame(drawSignal);
      };
      this.signalFrame = window.requestAnimationFrame(drawSignal);
    } catch {
      this.callbacks?.onSignal(EMPTY_SIGNAL);
    }
  }

  private cleanupCapture() {
    if (this.signalFrame !== null) window.cancelAnimationFrame(this.signalFrame);
    this.signalFrame = null;
    this.lastSignalFrame = 0;
    const audioContext = this.audioContext;
    this.audioContext = null;
    if (audioContext) void audioContext.close().catch(() => undefined);
    this.callbacks?.onSignal(EMPTY_SIGNAL);
    for (const track of this.stream?.getTracks() ?? []) track.stop();
    this.recorder = null;
    this.stream = null;
    this.chunks = [];
    this.destination = null;
    const release = this.releaseRecordingLock;
    this.releaseRecordingLock = null;
    release?.();
  }
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
