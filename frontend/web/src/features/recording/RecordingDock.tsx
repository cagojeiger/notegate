import { ChevronDown, Mic, Pause, Play, Square, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "../../shared/ui";
import {
  type RecordingStatus,
  useAudioRecordingActions,
  useAudioRecordingSignal,
  useAudioRecordingState
} from "./AudioRecordingContext";

const EMPTY_SIGNAL = Array.from({ length: 12 }, () => 0);

export function RecordingDock() {
  const state = useAudioRecordingState();
  const signal = useAudioRecordingSignal();
  const {
    discardRecording,
    pauseRecording,
    resumeRecording,
    stopRecording
  } = useAudioRecordingActions();
  const [collapsed, setCollapsed] = useState(false);
  const [now, setNow] = useState(() => performance.now());

  useEffect(() => {
    if (state.status === "idle") setCollapsed(false);
  }, [state.status]);

  useEffect(() => {
    if (state.status !== "recording" && state.status !== "paused") return;
    setNow(performance.now());
    const timer = window.setInterval(() => setNow(performance.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [state.status]);

  if (state.status === "idle") return null;

  const recordedDurationMs = state.recordedDurationMs + (
    state.status === "recording" && state.activeSegmentStartedAt !== null
      ? Math.max(0, now - state.activeSegmentStartedAt)
      : 0
  );
  const pausedDurationMs = state.pausedDurationMs + (
    state.status === "paused" && state.activePauseStartedAt !== null
      ? Math.max(0, now - state.activePauseStartedAt)
      : 0
  );
  const captureActive = state.status === "recording" || state.status === "paused";
  const paused = state.status === "paused";

  return (
    <section
      aria-label="Audio recorder"
      className="pointer-events-auto shrink-0 border-t border-seam bg-surface text-text md:overflow-hidden md:rounded-lg md:border md:border-border md:shadow-[var(--ng-focus-shadow)]"
    >
      <span className="sr-only" role="status">{recordingStatusAnnouncement(state.status)}</span>
      <button
        type="button"
        onClick={() => setCollapsed((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left outline-none hover:bg-[var(--ng-hover)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
        aria-label={`${collapsed ? "Expand" : "Collapse"} recorder`}
        aria-expanded={!collapsed}
        aria-controls="audio-recorder-details"
      >
        <span className={`relative grid size-7 shrink-0 place-items-center rounded-full ${paused ? "bg-warning/10 text-warning" : "bg-danger/10 text-danger"}`}>
          {state.status === "recording" ? <span className="absolute inset-0 rounded-full bg-danger/10 motion-safe:animate-pulse" aria-hidden="true" /> : null}
          <Mic size={15} className="relative" aria-hidden="true" />
        </span>
        <span className="min-w-0 flex-1 truncate text-sm font-medium">
          {state.status === "requesting" ? "Starting recording…" : null}
          {state.status === "recording" ? `Recording · ${formatDuration(recordedDurationMs)}` : null}
          {state.status === "paused" ? `Paused · ${formatDuration(recordedDurationMs)}` : null}
          {state.status === "stopping" ? "Preparing recording…" : null}
        </span>
        {captureActive ? (
          <SignalBars levels={paused ? EMPTY_SIGNAL : signal} paused={paused} />
        ) : null}
        <ChevronDown
          size={15}
          className={`shrink-0 text-muted transition ${collapsed ? "-rotate-90" : ""}`}
          aria-hidden="true"
        />
      </button>

      {!collapsed ? (
        <div id="audio-recorder-details" className="border-t border-seam px-3 py-2.5">
          <div className="min-w-0">
            <div className="truncate text-xs font-medium" title={state.filename ?? undefined}>
              {state.filename ?? "Audio recording"}
            </div>
            <div className="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-1.5 text-xs text-muted">
              <span className="truncate" title={state.destinationPath ?? "/"}>{state.destinationPath ?? "/"}</span>
              {captureActive ? (
                <>
                  <span aria-hidden="true">·</span>
                  <span>{formatSegmentCount(state.segmentCount)}</span>
                  <span aria-hidden="true">·</span>
                  <span className="tabular-nums">{formatDuration(pausedDurationMs)} paused</span>
                </>
              ) : null}
            </div>
          </div>
          {captureActive ? (
            <div className="mt-3 flex flex-wrap justify-end gap-2">
              <Button
                size="sm"
                variant="secondary"
                onClick={paused ? resumeRecording : pauseRecording}
              >
                {paused ? <Play size={14} aria-hidden="true" /> : <Pause size={14} aria-hidden="true" />}
                {paused ? "Resume" : "Pause"}
              </Button>
              <Button size="sm" variant="ghost" onClick={discardRecording}>
                <Trash2 size={14} aria-hidden="true" /> Discard
              </Button>
              <Button size="sm" onClick={stopRecording}>
                <Square size={14} aria-hidden="true" /> Stop &amp; save
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

function SignalBars({ levels, paused }: { levels: number[]; paused: boolean }) {
  return (
    <div
      className={`flex h-7 w-16 shrink-0 items-center justify-center gap-0.5 sm:w-20 ${paused ? "opacity-40" : ""}`}
      aria-label={paused ? "Microphone input paused" : "Microphone input level"}
    >
      {levels.map((level, index) => (
        <span
          // The number and order of bars are stable for the life of the recorder.
          key={index}
          className="w-1 rounded-full bg-primary transition-[height] duration-75 motion-reduce:transition-none"
          style={{ height: `${Math.max(3, Math.round(level * 22))}px` }}
          aria-hidden="true"
        />
      ))}
    </div>
  );
}

function formatSegmentCount(count: number): string {
  return `${count} ${count === 1 ? "segment" : "segments"}`;
}

function recordingStatusAnnouncement(status: Exclude<RecordingStatus, "idle">): string {
  if (status === "requesting") return "Starting audio recording";
  if (status === "recording") return "Audio recording in progress";
  if (status === "paused") return "Audio recording paused";
  return "Saving audio recording";
}

function formatDuration(totalMilliseconds: number): string {
  const totalSeconds = Math.max(0, Math.floor(totalMilliseconds / 1_000));
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${twoDigits(minutes)}:${twoDigits(seconds)}`
    : `${minutes}:${twoDigits(seconds)}`;
}

function twoDigits(value: number): string {
  return String(value).padStart(2, "0");
}
