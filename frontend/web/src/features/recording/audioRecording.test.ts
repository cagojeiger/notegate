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
    }, "audio/webm;codecs=opus", 63_500, {
      startedAt: Date.UTC(2026, 7, 11, 1, 2, 3),
      wallDurationMs: 12_000,
      recordedDurationMs: 9_000,
      segments: [
        {
          index: 0,
          wallStartOffsetMs: 0,
          wallEndOffsetMs: 5_000,
          mediaStartOffsetMs: 0,
          mediaEndOffsetMs: 5_000
        },
        {
          index: 1,
          wallStartOffsetMs: 8_000,
          wallEndOffsetMs: 12_000,
          mediaStartOffsetMs: 5_000,
          mediaEndOffsetMs: 9_000
        }
      ]
    })).toEqual({
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
      },
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
    });
  });

  it("bounds interruption segments below the node metadata size limit", () => {
    const metadata = recordingNodeMetadata({}, "audio/webm;codecs=opus", 64_000, {
      startedAt: 0,
      wallDurationMs: 100_000,
      recordedDurationMs: 70_000,
      segments: Array.from({ length: 80 }, (_, index) => ({
        index,
        wallStartOffsetMs: index * 1_000,
        wallEndOffsetMs: index * 1_000 + 800,
        mediaStartOffsetMs: index * 800,
        mediaEndOffsetMs: (index + 1) * 800
      }))
    });

    expect(metadata.recording_timeline).toEqual(expect.objectContaining({
      segment_count: 80,
      segments_included_count: 64,
      segments_omitted_count: 16
    }));
    expect(metadata.recording_segments).toEqual(expect.arrayContaining([
      expect.objectContaining({ index: 0 }),
      expect.objectContaining({ index: 79 })
    ]));
    expect((metadata.recording_segments as unknown[]).map((segment) => (
      segment as { index: number }
    ).index)).toEqual([
      ...Array.from({ length: 32 }, (_, index) => index),
      ...Array.from({ length: 32 }, (_, index) => index + 48)
    ]);
    expect(new TextEncoder().encode(JSON.stringify(metadata)).byteLength).toBeLessThan(16_384);
    expect(metadataDepth(metadata)).toBeLessThanOrEqual(4);
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

function metadataDepth(value: unknown, depth = 1): number {
  if (value === null || typeof value !== "object") return depth;
  const children = Array.isArray(value) ? value : Object.values(value);
  return children.reduce((maximum, child) => (
    Math.max(maximum, metadataDepth(child, depth + 1))
  ), depth);
}
