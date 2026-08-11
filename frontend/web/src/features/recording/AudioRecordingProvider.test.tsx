import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useUiStore } from "../../stores/uiStore";
import { AudioRecordingProvider, useAudioRecordingActions, useAudioRecordingState } from "./AudioRecordingProvider";
import { MICROPHONE_CAPTURE_DEFAULTS } from "./audioRecording";

const mocks = vi.hoisted(() => ({
  startUpload: vi.fn(),
  tasks: [] as Array<{ id: string; status: string }>,
  getUserMedia: vi.fn(),
  releaseWakeLock: vi.fn(),
  requestRecordingLock: vi.fn(),
  requestWakeLock: vi.fn()
}));

vi.mock("../uploads/UploadProvider", () => ({
  useUploadActions: () => ({ startUpload: mocks.startUpload }),
  useUploadManager: () => ({ tasks: mocks.tasks })
}));

describe("AudioRecordingProvider", () => {
  beforeEach(() => {
    MockMediaRecorder.instances = [];
    useUiStore.setState(useUiStore.getInitialState(), true);
    mocks.tasks = [];
    mocks.startUpload.mockReset().mockReturnValue("upload-recording");
    mocks.releaseWakeLock.mockReset().mockResolvedValue(undefined);
    mocks.requestWakeLock.mockReset().mockResolvedValue({ release: mocks.releaseWakeLock });
    mocks.requestRecordingLock.mockReset().mockImplementation((
      _name: string,
      _options: LockOptions,
      callback: (lock: Lock | null) => Promise<void>
    ) => callback({ name: "notegate:audio-recording", mode: "exclusive" } as Lock));
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: true });
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    Object.defineProperty(navigator, "wakeLock", {
      configurable: true,
      value: { request: mocks.requestWakeLock }
    });
    Object.defineProperty(navigator, "locks", {
      configurable: true,
      value: { request: mocks.requestRecordingLock }
    });
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: {
        getUserMedia: mocks.getUserMedia.mockReset().mockResolvedValue({
          getTracks: () => [{ stop: vi.fn() }],
          getAudioTracks: () => [{
            stop: vi.fn(),
            getSettings: () => ({
              sampleRate: 48_000,
              sampleSize: 16,
              channelCount: 1,
              echoCancellation: false,
              noiseSuppression: false,
              autoGainControl: false
            })
          }]
        })
      }
    });
    vi.stubGlobal("MediaRecorder", MockMediaRecorder);
  });

  it("records, uploads, and keeps the screen awake until upload completion", async () => {
    const { result, rerender } = renderHook(() => ({
      actions: useAudioRecordingActions(),
      state: useAudioRecordingState()
    }), { wrapper });

    await act(async () => {
      await result.current.actions.startRecording({
        spaceId: "space-1",
        spaceName: "Meetings",
        parentNodeId: "folder-1",
        destinationPath: "/Meetings"
      });
    });

    expect(result.current.state.status).toBe("recording");
    expect(mocks.requestWakeLock).toHaveBeenCalledWith("screen");
    expect(mocks.getUserMedia).toHaveBeenCalledWith({
      audio: MICROPHONE_CAPTURE_DEFAULTS
    });

    act(() => result.current.actions.stopRecording());

    await waitFor(() => expect(mocks.startUpload).toHaveBeenCalledWith(expect.objectContaining({
      spaceId: "space-1",
      parentNodeId: "folder-1",
      destinationPath: "/Meetings",
      name: expect.stringMatching(/-record\.webm$/),
      file: expect.any(File),
      nodeMetadata: expect.objectContaining({
        type: "audio_recording",
        recording: expect.objectContaining({
          profile_id: "notegate-meeting-llm-v1",
          actual: expect.objectContaining({
            mime_type: "audio/webm;codecs=opus",
            audio_bits_per_second: 64_000,
            sample_rate: 48_000,
            channel_count: 1
          })
        })
      })
    })));
    expect(result.current.state.status).toBe("idle");
    expect(mocks.releaseWakeLock).not.toHaveBeenCalled();

    mocks.tasks = [{ id: "upload-recording", status: "completed" }];
    rerender();

    await waitFor(() => expect(result.current.state.status).toBe("idle"));
    expect(mocks.releaseWakeLock).toHaveBeenCalledOnce();
    expect(useUiStore.getState().toast).toBe("Recording saved");
  });

  it("keeps the same capture session and wake lock across pause and resume", async () => {
    const { result } = renderHook(() => ({
      actions: useAudioRecordingActions(),
      state: useAudioRecordingState()
    }), { wrapper });

    await act(async () => {
      await result.current.actions.startRecording({
        spaceId: "space-1",
        spaceName: "Meetings",
        parentNodeId: "folder-1",
        destinationPath: "/Meetings"
      });
    });

    const recorder = MockMediaRecorder.instances[0];
    act(() => result.current.actions.pauseRecording());

    expect(result.current.state.status).toBe("paused");
    expect(recorder.pauseCalls).toBe(1);
    expect(mocks.releaseWakeLock).not.toHaveBeenCalled();

    act(() => result.current.actions.resumeRecording());

    expect(result.current.state.status).toBe("recording");
    expect(recorder.resumeCalls).toBe(1);
    expect(mocks.getUserMedia).toHaveBeenCalledOnce();
    expect(mocks.releaseWakeLock).not.toHaveBeenCalled();

    act(() => result.current.actions.pauseRecording());
    act(() => result.current.actions.stopRecording());

    await waitFor(() => expect(mocks.startUpload).toHaveBeenCalledOnce());
    expect(mocks.startUpload).toHaveBeenCalledWith(expect.objectContaining({
      nodeMetadata: expect.objectContaining({
        recording_timeline: expect.objectContaining({
          segment_count: 2,
          segments_included_count: 2,
          segments_omitted_count: 0
        }),
        recording_segments: expect.arrayContaining([
          expect.objectContaining({ index: 0 }),
          expect.objectContaining({ index: 1 })
        ])
      })
    }));
  });

  it("allows the next recording while the previous file is uploading", async () => {
    const { result } = renderHook(() => ({
      actions: useAudioRecordingActions(),
      state: useAudioRecordingState()
    }), { wrapper });
    const destination = {
      spaceId: "space-1",
      spaceName: "Meetings",
      parentNodeId: "root-1",
      destinationPath: "/"
    };

    await act(async () => { await result.current.actions.startRecording(destination); });
    act(() => result.current.actions.stopRecording());
    expect(result.current.state.status).toBe("idle");

    mocks.tasks = [{ id: "upload-recording", status: "uploading" }];
    await act(async () => { await result.current.actions.startRecording(destination); });

    expect(result.current.state.status).toBe("recording");
    expect(mocks.getUserMedia).toHaveBeenCalledTimes(2);
  });

  it("does not request the microphone when another tab owns the recording lock", async () => {
    mocks.requestRecordingLock.mockImplementationOnce((
      _name: string,
      _options: LockOptions,
      callback: (lock: Lock | null) => Promise<void>
    ) => callback(null));
    const { result } = renderHook(() => ({
      actions: useAudioRecordingActions(),
      state: useAudioRecordingState()
    }), { wrapper });

    await act(async () => {
      await result.current.actions.startRecording({
        spaceId: "space-1",
        spaceName: "Meetings",
        parentNodeId: "root-1",
        destinationPath: "/"
      });
    });

    expect(result.current.state.status).toBe("idle");
    expect(mocks.getUserMedia).not.toHaveBeenCalled();
    expect(useUiStore.getState().toast).toBe("Audio is already recording in another NoteGate tab");
  });
});

function wrapper({ children }: { children: ReactNode }) {
  return <AudioRecordingProvider>{children}</AudioRecordingProvider>;
}

class MockMediaRecorder extends EventTarget {
  static instances: MockMediaRecorder[] = [];

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
    this.dispatchEvent(new Event("pause"));
  }

  resume() {
    if (this.state !== "paused") return;
    this.resumeCalls += 1;
    this.state = "recording";
    this.dispatchEvent(new Event("resume"));
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
