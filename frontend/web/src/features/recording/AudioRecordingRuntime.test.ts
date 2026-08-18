import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RecordingDestination } from "./AudioRecordingContext";
import { AudioRecordingRuntime } from "./AudioRecordingRuntime";

const finalizationMocks = vi.hoisted(() => ({
  finalizeRecordedAudio: vi.fn<(blob: Blob) => Promise<Blob>>()
}));

vi.mock("./finalizeRecordedAudio", () => finalizationMocks);

const destination: RecordingDestination = {
  spaceId: "space-1",
  spaceName: "Meetings",
  parentNodeId: "folder-1",
  destinationPath: "/Meetings"
};

describe("AudioRecordingRuntime", () => {
  beforeEach(() => {
    MockMediaRecorder.instances = [];
    MockMediaRecorder.deferStateEvents = false;
    finalizationMocks.finalizeRecordedAudio.mockReset().mockImplementation(async (blob) => blob);
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: true });
    Object.defineProperty(navigator, "locks", {
      configurable: true,
      value: {
        request: vi.fn((
          _name: string,
          _options: LockOptions,
          callback: (lock: Lock | null) => Promise<void>
        ) => callback({ name: "notegate:audio-recording", mode: "exclusive" } as Lock))
      }
    });
    vi.stubGlobal("MediaRecorder", MockMediaRecorder);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("discards paused audio without emitting an upload", async () => {
    const track = mockTrack();
    setMediaStream(mockStream(track));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    await runtime.start(destination, callbacks);
    runtime.pause();
    runtime.discard();

    expect(callbacks.onCaptured).not.toHaveBeenCalled();
    expect(callbacks.onState).toHaveBeenLastCalledWith({
      status: "idle",
      startedAt: null,
      activeSegmentStartedAt: null,
      activePauseStartedAt: null,
      recordedDurationMs: 0,
      pausedDurationMs: 0,
      segmentCount: 0,
      filename: null,
      destinationPath: null
    });
    expect(track.stop).toHaveBeenCalledOnce();
  });

  it("cleans up a recorder error even when a stop event follows", async () => {
    const track = mockTrack();
    setMediaStream(mockStream(track));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    await runtime.start(destination, callbacks);
    const recorder = MockMediaRecorder.instances[0];
    recorder.dispatchEvent(new Event("error"));
    recorder.dispatchEvent(new Event("stop"));

    expect(callbacks.onToast).toHaveBeenCalledWith("Recording stopped unexpectedly");
    expect(callbacks.onCaptured).not.toHaveBeenCalled();
    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({ status: "idle" }));
    expect(track.stop).toHaveBeenCalledOnce();
  });

  it("stops a late microphone stream after disposal", async () => {
    const track = mockTrack();
    let resolveStream: (stream: MediaStream) => void = () => undefined;
    const streamRequest = new Promise<MediaStream>((resolve) => {
      resolveStream = resolve;
    });
    const getUserMedia = setMediaStream(streamRequest);
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    const start = runtime.start(destination, callbacks);
    await vi.waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    runtime.dispose();
    resolveStream(mockStream(track));
    await start;

    expect(track.stop).toHaveBeenCalledOnce();
    expect(callbacks.onCaptured).not.toHaveBeenCalled();
    expect(MockMediaRecorder.instances).toHaveLength(0);
  });

  it("finalizes a WebM recording before upload", async () => {
    const monotonicNow = vi.spyOn(performance, "now").mockReturnValue(1_000);
    setMediaStream(mockStream(mockTrack()));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();
    const finalizedBlob = new Blob(["finalized audio"], { type: "audio/webm;codecs=opus" });
    finalizationMocks.finalizeRecordedAudio.mockResolvedValue(finalizedBlob);

    await runtime.start(destination, callbacks);
    monotonicNow.mockReturnValue(6_000);
    runtime.stop();

    await vi.waitFor(() => expect(callbacks.onCaptured).toHaveBeenCalledOnce());
    expect(finalizationMocks.finalizeRecordedAudio).toHaveBeenCalledWith(
      expect.objectContaining({ type: "audio/webm;codecs=opus" })
    );
    const capture = callbacks.onCaptured.mock.calls[0]?.[0];
    expect(capture?.file).toEqual(expect.objectContaining({
      name: expect.stringMatching(/-record\.webm$/),
      size: finalizedBlob.size,
      type: "audio/webm;codecs=opus"
    }));
  });

  it("preserves the original recording when playback optimization fails", async () => {
    const monotonicNow = vi.spyOn(performance, "now").mockReturnValue(1_000);
    setMediaStream(mockStream(mockTrack()));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();
    finalizationMocks.finalizeRecordedAudio.mockRejectedValue(new Error("invalid WebM"));

    await runtime.start(destination, callbacks);
    monotonicNow.mockReturnValue(3_000);
    runtime.stop();

    await vi.waitFor(() => expect(callbacks.onCaptured).toHaveBeenCalledOnce());
    expect(callbacks.onToast).toHaveBeenCalledWith(
      "Could not optimize audio playback; uploading the original recording"
    );
    expect(callbacks.onCaptured.mock.calls[0]?.[0].file.size).toBe(
      new Blob(["recorded audio"]).size
    );
  });

  it("pauses and resumes one recorder while preserving recorded-time segments", async () => {
    const startedAt = Date.UTC(2026, 7, 11, 1, 2, 3);
    vi.spyOn(Date, "now").mockReturnValue(startedAt);
    const monotonicNow = vi.spyOn(performance, "now").mockReturnValue(1_000);
    const track = mockTrack();
    setMediaStream(mockStream(track));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    await runtime.start(destination, callbacks);
    const recorder = MockMediaRecorder.instances[0];

    monotonicNow.mockReturnValue(6_000);
    callbacks.onSignal.mockClear();
    runtime.pause();

    expect(recorder.pauseCalls).toBe(1);
    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({
      status: "paused",
      activeSegmentStartedAt: null,
      activePauseStartedAt: 6_000,
      recordedDurationMs: 5_000,
      pausedDurationMs: 0,
      segmentCount: 1
    }));
    expect(callbacks.onSignal).toHaveBeenLastCalledWith(Array.from({ length: 12 }, () => 0));

    monotonicNow.mockReturnValue(9_000);
    runtime.resume();

    expect(recorder.resumeCalls).toBe(1);
    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({
      status: "recording",
      activeSegmentStartedAt: 9_000,
      activePauseStartedAt: null,
      recordedDurationMs: 5_000,
      pausedDurationMs: 3_000,
      segmentCount: 2
    }));

    monotonicNow.mockReturnValue(13_000);
    runtime.stop();

    await vi.waitFor(() => expect(callbacks.onCaptured).toHaveBeenCalledOnce());
    expect(callbacks.onCaptured).toHaveBeenCalledOnce();
    expect(callbacks.onCaptured).toHaveBeenCalledWith(expect.objectContaining({
      nodeMetadata: expect.objectContaining({
        recording_timeline: {
          started_at: "2026-08-11T01:02:03.000Z",
          ended_at: "2026-08-11T01:02:15.000Z",
          wall_duration_ms: 12_000,
          recorded_duration_ms: 9_000,
          paused_duration_ms: 3_000,
          segment_count: 2,
          segments_included_count: 2,
          segments_omitted_count: 0
        },
        recording_segments: [
          {
            index: 0,
            wall_start_offset_ms: 0,
            wall_end_offset_ms: 5_000,
            media_start_offset_ms: 0,
            media_end_offset_ms: 5_000
          },
          {
            index: 1,
            wall_start_offset_ms: 8_000,
            wall_end_offset_ms: 12_000,
            media_start_offset_ms: 5_000,
            media_end_offset_ms: 9_000
          }
        ]
      })
    }));
    expect(track.stop).toHaveBeenCalledOnce();
  });

  it("stops and saves directly from a paused state", async () => {
    const monotonicNow = vi.spyOn(performance, "now").mockReturnValue(1_000);
    setMediaStream(mockStream(mockTrack()));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    await runtime.start(destination, callbacks);
    monotonicNow.mockReturnValue(5_000);
    runtime.pause();
    monotonicNow.mockReturnValue(8_000);
    runtime.stop();

    await vi.waitFor(() => expect(callbacks.onCaptured).toHaveBeenCalledOnce());
    expect(callbacks.onCaptured).toHaveBeenCalledWith(expect.objectContaining({
      nodeMetadata: expect.objectContaining({
        recording_timeline: expect.objectContaining({
          wall_duration_ms: 7_000,
          recorded_duration_ms: 4_000,
          paused_duration_ms: 3_000,
          segment_count: 1
        })
      })
    }));
  });

  it("changes public state only after recorder pause and resume events", async () => {
    MockMediaRecorder.deferStateEvents = true;
    setMediaStream(mockStream(mockTrack()));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    await runtime.start(destination, callbacks);
    const recorder = MockMediaRecorder.instances[0];
    runtime.pause();

    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({ status: "recording" }));
    recorder.dispatchEvent(new Event("pause"));
    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({ status: "paused" }));

    runtime.resume();
    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({ status: "paused" }));
    recorder.dispatchEvent(new Event("resume"));
    expect(callbacks.onState).toHaveBeenLastCalledWith(expect.objectContaining({ status: "recording" }));
    runtime.discard();
  });
});

function mockCallbacks() {
  return {
    acquireWakeLock: vi.fn().mockResolvedValue(true),
    onCaptured: vi.fn(),
    onSignal: vi.fn(),
    onState: vi.fn(),
    onToast: vi.fn()
  };
}

function mockTrack() {
  return {
    getSettings: () => ({ sampleRate: 48_000, channelCount: 1 }),
    stop: vi.fn()
  };
}

function mockStream(track: ReturnType<typeof mockTrack>) {
  return {
    getAudioTracks: () => [track],
    getTracks: () => [track]
  } as unknown as MediaStream;
}

function setMediaStream(stream: MediaStream | Promise<MediaStream>) {
  const getUserMedia = vi.fn().mockReturnValue(Promise.resolve(stream));
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia }
  });
  return getUserMedia;
}

class MockMediaRecorder extends EventTarget {
  static instances: MockMediaRecorder[] = [];
  static deferStateEvents = false;

  static isTypeSupported(mimeType: string) {
    return mimeType === "audio/webm;codecs=opus";
  }

  readonly mimeType: string;
  readonly audioBitsPerSecond: number;
  state: RecordingState = "inactive";
  pauseCalls = 0;
  resumeCalls = 0;

  constructor(_stream: MediaStream, options?: MediaRecorderOptions) {
    super();
    this.mimeType = options?.mimeType ?? "audio/webm";
    this.audioBitsPerSecond = options?.audioBitsPerSecond ?? 0;
    MockMediaRecorder.instances.push(this);
  }

  start() {
    this.state = "recording";
  }

  pause() {
    if (this.state !== "recording") return;
    this.pauseCalls += 1;
    this.state = "paused";
    if (!MockMediaRecorder.deferStateEvents) this.dispatchEvent(new Event("pause"));
  }

  resume() {
    if (this.state !== "paused") return;
    this.resumeCalls += 1;
    this.state = "recording";
    if (!MockMediaRecorder.deferStateEvents) this.dispatchEvent(new Event("resume"));
  }

  stop() {
    this.state = "inactive";
    const dataEvent = new Event("dataavailable");
    Object.defineProperty(dataEvent, "data", {
      value: new Blob(["recorded audio"], { type: this.mimeType })
    });
    this.dispatchEvent(dataEvent);
    this.dispatchEvent(new Event("stop"));
  }
}
