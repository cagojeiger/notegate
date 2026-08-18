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
  type RecordingSegmentTiming,
  recordingSupport
} from "./audioRecording";
import { finalizeRecordedAudio } from "./finalizeRecordedAudio";

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
  private analyser: AnalyserNode | null = null;
  private frequencyData: Uint8Array<ArrayBuffer> | null = null;
  private signalFrame: number | null = null;
  private lastSignalFrame = 0;
  private chunks: Blob[] = [];
  private destination: RecordingDestination | null = null;
  private discardRequested = false;
  private releaseRecordingLock: (() => void) | null = null;
  private callbacks: RecordingRuntimeCallbacks | null = null;
  private disposed = false;
  private startedAt: number | null = null;
  private startedAtMonotonic: number | null = null;
  private endedAtMonotonic: number | null = null;
  private filename: string | null = null;
  private activeSegmentStartedAt: number | null = null;
  private activePauseStartedAt: number | null = null;
  private recordedDurationMs = 0;
  private pausedDurationMs = 0;
  private segments: RecordingSegmentTiming[] = [];

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
      const mimeType = recorder.mimeType || RECORDING_FORMAT.mimeType;

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
      recorder.addEventListener("pause", () => this.handlePause(recorder));
      recorder.addEventListener("resume", () => this.handleResume(recorder));
      recorder.addEventListener("stop", () => {
        void this.finishCapture(recorder, audioTrack, mimeType);
      }, { once: true });

      const awake = await callbacks.acquireWakeLock();
      if (!awake) callbacks.onToast("Keep NoteGate open until the recording upload finishes");

      const startedAt = Date.now();
      const startedAtMonotonic = performance.now();
      this.startedAt = startedAt;
      this.startedAtMonotonic = startedAtMonotonic;
      this.endedAtMonotonic = null;
      this.filename = recordingFilename(new Date(startedAt), mimeType);
      this.activeSegmentStartedAt = startedAtMonotonic;
      this.activePauseStartedAt = null;
      this.recordedDurationMs = 0;
      this.pausedDurationMs = 0;
      this.segments = [];

      this.startSignal(stream);
      recorder.start(5_000);
      this.setState("recording");
    } catch (error) {
      this.cleanupCapture();
      this.setState("idle");
      const denied = error instanceof DOMException
        && (error.name === "NotAllowedError" || error.name === "SecurityError");
      callbacks.onToast(denied ? "Microphone permission is required" : "Could not start audio recording");
    }
  }

  pause() {
    if (this.status !== "recording" || this.recorder?.state !== "recording") return;
    this.recorder.pause();
  }

  resume() {
    if (this.status !== "paused" || this.recorder?.state !== "paused") return;
    this.recorder.resume();
  }

  stop() {
    if (!this.recorder || this.recorder.state === "inactive") return;
    this.discardRequested = false;
    this.finishTimeline(performance.now());
    this.pauseSignal();
    this.setState("stopping");
    this.recorder.stop();
  }

  discard() {
    if (!this.recorder || this.recorder.state === "inactive") return;
    this.discardRequested = true;
    this.finishTimeline(performance.now());
    this.pauseSignal();
    this.setState("stopping");
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

  private handlePause(recorder: MediaRecorder) {
    if (this.recorder !== recorder || this.status !== "recording") return;
    const now = performance.now();
    this.closeActiveSegment(now);
    this.activePauseStartedAt = now;
    this.pauseSignal();
    this.setState("paused");
  }

  private handleResume(recorder: MediaRecorder) {
    if (this.recorder !== recorder || this.status !== "paused") return;
    const now = performance.now();
    this.closeActivePause(now);
    this.activeSegmentStartedAt = now;
    this.setState("recording");
    this.resumeSignal();
  }

  private async finishCapture(
    recorder: MediaRecorder,
    audioTrack: MediaStreamTrack | undefined,
    fallbackMimeType: string
  ) {
    if (this.disposed) {
      this.cleanupCapture();
      return;
    }
    this.finishTimeline(performance.now());
    const callbacks = this.callbacks;
    const destination = this.destination;
    const shouldDiscard = this.discardRequested;
    const startedAt = this.startedAt ?? Date.now();
    const recordedMimeType = recorder.mimeType || this.chunks[0]?.type || fallbackMimeType;
    const captureSettings = audioTrack?.getSettings() ?? {};
    const recordedDurationMs = this.recordedDurationMs;
    const recordedFile = new File(
      this.chunks,
      this.filename ?? recordingFilename(new Date(startedAt), recordedMimeType),
      { type: recordedMimeType }
    );
    const nodeMetadata = recordingNodeMetadata(
      captureSettings,
      recordedMimeType,
      recorder.audioBitsPerSecond,
      {
        startedAt,
        wallDurationMs: this.wallDurationMs(),
        recordedDurationMs,
        segments: this.segments
      }
    );

    if (!callbacks || shouldDiscard || !destination) {
      this.cleanupCapture();
      this.setState("idle");
      return;
    }
    if (recordedFile.size === 0) {
      this.cleanupCapture();
      this.setState("idle");
      callbacks.onToast("No audio was captured");
      return;
    }

    this.stopCaptureResources();
    let file = recordedFile;
    try {
      const finalizedBlob = await finalizeRecordedAudio(recordedFile);
      if (finalizedBlob !== recordedFile) {
        file = new File([finalizedBlob], recordedFile.name, {
          type: recordedFile.type,
          lastModified: recordedFile.lastModified
        });
      }
    } catch {
      callbacks.onToast("Could not optimize audio playback; uploading the original recording");
    }
    if (this.disposed) return;

    this.cleanupCapture();
    this.setState("idle");
    callbacks.onCaptured({ destination, file, nodeMetadata });
  }

  private finishTimeline(now: number) {
    if (this.endedAtMonotonic !== null || this.startedAtMonotonic === null) return;
    this.closeActiveSegment(now);
    this.closeActivePause(now);
    this.endedAtMonotonic = now;
  }

  private closeActiveSegment(now: number) {
    if (this.activeSegmentStartedAt === null || this.startedAtMonotonic === null) return;
    const wallStartOffsetMs = this.activeSegmentStartedAt - this.startedAtMonotonic;
    const wallEndOffsetMs = Math.max(wallStartOffsetMs, now - this.startedAtMonotonic);
    const mediaStartOffsetMs = this.recordedDurationMs;
    const mediaEndOffsetMs = mediaStartOffsetMs + Math.max(0, now - this.activeSegmentStartedAt);
    this.segments.push({
      index: this.segments.length,
      wallStartOffsetMs,
      wallEndOffsetMs,
      mediaStartOffsetMs,
      mediaEndOffsetMs
    });
    this.recordedDurationMs = mediaEndOffsetMs;
    this.activeSegmentStartedAt = null;
  }

  private closeActivePause(now: number) {
    if (this.activePauseStartedAt === null) return;
    this.pausedDurationMs += Math.max(0, now - this.activePauseStartedAt);
    this.activePauseStartedAt = null;
  }

  private wallDurationMs() {
    if (this.startedAtMonotonic === null) return 0;
    return Math.max(
      0,
      (this.endedAtMonotonic ?? performance.now()) - this.startedAtMonotonic
    );
  }

  private finishRequest(message: string) {
    const callbacks = this.callbacks;
    this.cleanupCapture();
    this.setState("idle");
    callbacks?.onToast(message);
  }

  private setState(status: RecordingStatus) {
    this.status = status;
    this.callbacks?.onState({
      status,
      startedAt: this.startedAt,
      activeSegmentStartedAt: status === "recording" ? this.activeSegmentStartedAt : null,
      activePauseStartedAt: status === "paused" ? this.activePauseStartedAt : null,
      recordedDurationMs: this.recordedDurationMs,
      pausedDurationMs: this.pausedDurationMs,
      segmentCount: this.segments.length + (this.activeSegmentStartedAt === null ? 0 : 1),
      filename: this.filename,
      destinationPath: this.destination?.destinationPath ?? null
    });
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
      this.analyser = analyser;
      this.frequencyData = new Uint8Array(analyser.frequencyBinCount);
      void audioContext.resume().catch(() => undefined);
      this.resumeSignal();
    } catch {
      this.callbacks?.onSignal(EMPTY_SIGNAL);
    }
  }

  private resumeSignal() {
    if (!this.analyser || !this.frequencyData || this.signalFrame !== null) return;
    const drawSignal = (timestamp: number) => {
      this.signalFrame = null;
      if (this.status !== "recording" || !this.analyser || !this.frequencyData) return;
      if (timestamp - this.lastSignalFrame >= SIGNAL_FRAME_INTERVAL_MS) {
        this.lastSignalFrame = timestamp;
        this.analyser.getByteFrequencyData(this.frequencyData);
        this.callbacks?.onSignal(sampleSignal(this.frequencyData));
      }
      this.signalFrame = window.requestAnimationFrame(drawSignal);
    };
    this.signalFrame = window.requestAnimationFrame(drawSignal);
  }

  private pauseSignal() {
    if (this.signalFrame !== null) window.cancelAnimationFrame(this.signalFrame);
    this.signalFrame = null;
    this.lastSignalFrame = 0;
    this.callbacks?.onSignal(EMPTY_SIGNAL);
  }

  private cleanupCapture() {
    this.stopCaptureResources();
    this.chunks = [];
    this.destination = null;
    this.startedAt = null;
    this.startedAtMonotonic = null;
    this.endedAtMonotonic = null;
    this.filename = null;
    this.activeSegmentStartedAt = null;
    this.activePauseStartedAt = null;
    this.recordedDurationMs = 0;
    this.pausedDurationMs = 0;
    this.segments = [];
    const release = this.releaseRecordingLock;
    this.releaseRecordingLock = null;
    release?.();
  }

  private stopCaptureResources() {
    this.pauseSignal();
    this.analyser = null;
    this.frequencyData = null;
    const audioContext = this.audioContext;
    this.audioContext = null;
    if (audioContext) void audioContext.close().catch(() => undefined);
    for (const track of this.stream?.getTracks() ?? []) track.stop();
    this.recorder = null;
    this.stream = null;
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
