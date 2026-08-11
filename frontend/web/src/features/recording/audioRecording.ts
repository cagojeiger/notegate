export type RecordingSupport = {
  supported: boolean;
  reason: string | null;
};

export const RECORDING_PROFILE_ID = "notegate-meeting-llm-v1";
export const RECORDING_FORMAT = {
  mimeType: "audio/webm;codecs=opus",
  audioBitsPerSecond: 64_000
} as const satisfies MediaRecorderOptions;
export const MICROPHONE_CAPTURE_DEFAULTS = {
  channelCount: { ideal: 1 },
  sampleRate: { ideal: 48_000 },
  echoCancellation: { ideal: false },
  noiseSuppression: { ideal: false },
  autoGainControl: { ideal: false }
} as const satisfies MediaTrackConstraints;

export function recordingSupport(): RecordingSupport {
  if (!window.isSecureContext) {
    return {
      supported: false,
      reason: "Audio recording requires HTTPS"
    };
  }
  if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === "undefined") {
    return {
      supported: false,
      reason: "This browser does not support audio recording"
    };
  }
  if (
    typeof MediaRecorder.isTypeSupported !== "function"
    || !MediaRecorder.isTypeSupported(RECORDING_FORMAT.mimeType)
  ) {
    return {
      supported: false,
      reason: "This browser does not support WebM/Opus audio recording"
    };
  }
  if (!navigator.locks) {
    return {
      supported: false,
      reason: "This browser cannot safely coordinate audio recording across tabs"
    };
  }
  return {
    supported: true,
    reason: null
  };
}

export function recordingNodeMetadata(
  settings: MediaTrackSettings,
  actualMimeType: string,
  actualAudioBitsPerSecond: number
): Record<string, unknown> {
  return {
    type: "audio_recording",
    recording: {
      profile_id: RECORDING_PROFILE_ID,
      requested: {
        mime_type: RECORDING_FORMAT.mimeType,
        audio_bits_per_second: RECORDING_FORMAT.audioBitsPerSecond,
        sample_rate: 48_000,
        channel_count: 1,
        echo_cancellation: false,
        noise_suppression: false,
        auto_gain_control: false
      },
      actual: definedValues({
        mime_type: actualMimeType,
        audio_bits_per_second: actualAudioBitsPerSecond,
        sample_rate: settings.sampleRate,
        sample_size: settings.sampleSize,
        channel_count: settings.channelCount,
        echo_cancellation: settings.echoCancellation,
        noise_suppression: settings.noiseSuppression,
        auto_gain_control: settings.autoGainControl
      })
    }
  };
}

export function recordingFilename(date: Date, mimeType: string): string {
  const year = date.getFullYear();
  const month = twoDigits(date.getMonth() + 1);
  const day = twoDigits(date.getDate());
  const hour = twoDigits(date.getHours());
  const minute = twoDigits(date.getMinutes());
  const second = twoDigits(date.getSeconds());
  return `${year}-${month}-${day}-${hour}${minute}${second}-record.${recordingExtension(mimeType)}`;
}

export function recordingExtension(mimeType: string): string {
  const normalized = mimeType.toLowerCase();
  if (normalized.includes("mp4") || normalized.includes("m4a")) return "m4a";
  if (normalized.includes("ogg")) return "ogg";
  if (normalized.includes("wav")) return "wav";
  return "webm";
}

function twoDigits(value: number): string {
  return String(value).padStart(2, "0");
}

function definedValues(values: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(values).filter(([, value]) => value !== undefined));
}
