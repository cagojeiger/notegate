import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RecordingDestination } from "./AudioRecordingContext";
import { AudioRecordingRuntime } from "./AudioRecordingRuntime";

const destination: RecordingDestination = {
  spaceId: "space-1",
  spaceName: "Meetings",
  parentNodeId: "folder-1",
  destinationPath: "/Meetings"
};

describe("AudioRecordingRuntime", () => {
  beforeEach(() => {
    MockMediaRecorder.instances = [];
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

  it("discards captured audio without emitting an upload", async () => {
    const track = mockTrack();
    setMediaStream(mockStream(track));
    const runtime = new AudioRecordingRuntime();
    const callbacks = mockCallbacks();

    await runtime.start(destination, callbacks);
    runtime.discard();

    expect(callbacks.onCaptured).not.toHaveBeenCalled();
    expect(callbacks.onState).toHaveBeenLastCalledWith({
      status: "idle",
      startedAt: null,
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

  static isTypeSupported(mimeType: string) {
    return mimeType === "audio/webm;codecs=opus";
  }

  readonly mimeType: string;
  readonly audioBitsPerSecond: number;
  state: RecordingState = "inactive";

  constructor(_stream: MediaStream, options?: MediaRecorderOptions) {
    super();
    this.mimeType = options?.mimeType ?? "audio/webm";
    this.audioBitsPerSecond = options?.audioBitsPerSecond ?? 0;
    MockMediaRecorder.instances.push(this);
  }

  start() {
    this.state = "recording";
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
