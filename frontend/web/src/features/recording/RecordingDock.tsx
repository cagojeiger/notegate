import { Mic, Square, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "../../shared/ui";
import {
  useAudioRecordingActions,
  useAudioRecordingSignal,
  useAudioRecordingState
} from "./AudioRecordingProvider";

export function RecordingDock() {
  const state = useAudioRecordingState();
  const signal = useAudioRecordingSignal();
  const { discardRecording, stopRecording } = useAudioRecordingActions();
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (state.status !== "recording") return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [state.status]);

  if (state.status === "idle") return null;
  const elapsedSeconds = state.startedAt ? Math.max(0, Math.floor((now - state.startedAt) / 1_000)) : 0;

  return (
    <div className="flex min-h-12 flex-wrap items-center gap-x-3 gap-y-2 border-t border-seam bg-panel px-3 py-2 text-sm sm:flex-nowrap" role="status">
      <div className="flex min-w-0 flex-1 items-center gap-3">
        <span className="relative grid size-7 shrink-0 place-items-center rounded-full bg-danger/10 text-danger">
          <span className="absolute inset-0 rounded-full bg-danger/10 motion-safe:animate-pulse" aria-hidden="true" />
          <Mic size={15} className="relative" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium">
            {state.status === "requesting" ? "Starting recording…" : null}
            {state.status === "recording" ? `Recording · ${formatDuration(elapsedSeconds)}` : null}
            {state.status === "stopping" ? "Preparing recording…" : null}
          </div>
          <div className="truncate text-xs text-muted">
            {state.filename ?? "Audio recording"} · {state.destinationPath ?? "/"}
          </div>
        </div>
        {state.status === "recording" ? <SignalBars levels={signal} /> : null}
      </div>
      {state.status === "recording" ? (
        <div className="ml-auto flex w-full justify-end gap-2 sm:w-auto">
          <Button size="sm" variant="ghost" onClick={discardRecording}><Trash2 size={14} /> Discard</Button>
          <Button size="sm" onClick={stopRecording}><Square size={14} /> Stop &amp; save</Button>
        </div>
      ) : null}
    </div>
  );
}

function SignalBars({ levels }: { levels: number[] }) {
  return (
    <div className="flex h-7 w-20 shrink-0 items-center justify-center gap-0.5 sm:w-24" aria-label="Microphone input level">
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

function formatDuration(totalSeconds: number): string {
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
