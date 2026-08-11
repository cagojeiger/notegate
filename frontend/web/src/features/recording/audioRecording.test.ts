import { afterEach, describe, expect, it, vi } from "vitest";

import {
  MICROPHONE_CAPTURE_DEFAULTS,
  RECORDING_FORMAT,
  RECORDING_PROFILE_ID,
  recordingExtension,
  recordingFilename,
  recordingNodeMetadata,
  recordingSupport
} from "./audioRecording";

describe("audioRecording", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("uses one stable meeting capture profile", () => {
    expect(RECORDING_FORMAT).toEqual({
      mimeType: "audio/webm;codecs=opus",
      audioBitsPerSecond: 64_000
    });
    expect(MICROPHONE_CAPTURE_DEFAULTS).toEqual({
      channelCount: { ideal: 1 },
      sampleRate: { ideal: 48_000 },
      echoCancellation: { ideal: false },
      noiseSuppression: { ideal: false },
      autoGainControl: { ideal: false }
    });
  });

  it("records requested and actual capture settings as node metadata", () => {
    expect(recordingNodeMetadata({
      sampleRate: 44_100,
      sampleSize: 16,
      channelCount: 1,
      echoCancellation: true,
      noiseSuppression: false,
      autoGainControl: false
    }, "audio/webm;codecs=opus", 63_500)).toEqual({
      type: "audio_recording",
      recording: {
        profile_id: RECORDING_PROFILE_ID,
        requested: {
          mime_type: "audio/webm;codecs=opus",
          audio_bits_per_second: 64_000,
          sample_rate: 48_000,
          channel_count: 1,
          echo_cancellation: false,
          noise_suppression: false,
          auto_gain_control: false
        },
        actual: {
          mime_type: "audio/webm;codecs=opus",
          audio_bits_per_second: 63_500,
          sample_rate: 44_100,
          sample_size: 16,
          channel_count: 1,
          echo_cancellation: true,
          noise_suppression: false,
          auto_gain_control: false
        }
      }
    });
  });

  it("builds stable local-time filenames for browser audio containers", () => {
    const date = new Date(2026, 7, 11, 9, 7, 5);

    expect(recordingFilename(date, "audio/mp4;codecs=mp4a.40.2"))
      .toBe("2026-08-11-090705-record.m4a");
    expect(recordingExtension("audio/ogg;codecs=opus")).toBe("ogg");
    expect(recordingExtension("audio/webm;codecs=opus")).toBe("webm");
  });

  it("reports secure-context and browser API requirements", () => {
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: false });
    expect(recordingSupport()).toMatchObject({
      supported: false,
      reason: "Audio recording requires HTTPS"
    });

    Object.defineProperty(window, "isSecureContext", { configurable: true, value: true });
    expect(recordingSupport().supported).toBe(false);
  });

  it("does not fall back when the fixed WebM/Opus format is unavailable", () => {
    Object.defineProperty(window, "isSecureContext", { configurable: true, value: true });
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: { getUserMedia: vi.fn() }
    });
    Object.defineProperty(navigator, "locks", {
      configurable: true,
      value: { request: vi.fn() }
    });
    vi.stubGlobal("MediaRecorder", class {
      static isTypeSupported() {
        return false;
      }
    });

    expect(recordingSupport()).toEqual({
      supported: false,
      reason: "This browser does not support WebM/Opus audio recording"
    });
  });
});
