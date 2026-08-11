import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RecordingDock } from "./RecordingDock";

type MockStatus = "idle" | "requesting" | "recording" | "paused" | "stopping";

const mocks = vi.hoisted(() => ({
  discardRecording: vi.fn(),
  pauseRecording: vi.fn(),
  resumeRecording: vi.fn(),
  signal: Array.from({ length: 12 }, (_, index) => index / 11),
  state: {
    status: "recording" as MockStatus,
    startedAt: Date.now() as number | null,
    activeSegmentStartedAt: 1_000 as number | null,
    activePauseStartedAt: null as number | null,
    recordedDurationMs: 2_000,
    pausedDurationMs: 1_000,
    segmentCount: 2,
    filename: "2026-08-11-153045-record.m4a" as string | null,
    destinationPath: "/" as string | null
  },
  stopRecording: vi.fn()
}));

vi.mock("./AudioRecordingContext", () => ({
  useAudioRecordingActions: () => ({
    discardRecording: mocks.discardRecording,
    pauseRecording: mocks.pauseRecording,
    resumeRecording: mocks.resumeRecording,
    stopRecording: mocks.stopRecording
  }),
  useAudioRecordingSignal: () => mocks.signal,
  useAudioRecordingState: () => mocks.state
}));

describe("RecordingDock", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(performance, "now").mockReturnValue(4_000);
    mocks.discardRecording.mockReset();
    mocks.pauseRecording.mockReset();
    mocks.resumeRecording.mockReset();
    mocks.stopRecording.mockReset();
    mocks.state = {
      status: "recording",
      startedAt: Date.now(),
      activeSegmentStartedAt: 1_000,
      activePauseStartedAt: null,
      recordedDurationMs: 2_000,
      pausedDurationMs: 1_000,
      segmentCount: 2,
      filename: "2026-08-11-153045-record.m4a",
      destinationPath: "/"
    };
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("shows live capture details and keeps the compact header when collapsed", () => {
    render(<RecordingDock />);

    expect(screen.getByRole("status")).toHaveTextContent("Audio recording in progress");
    expect(screen.getByText("Recording · 0:05")).toBeInTheDocument();
    expect(screen.getByText("2026-08-11-153045-record.m4a")).toBeInTheDocument();
    expect(screen.getByText("2 segments")).toBeInTheDocument();
    expect(screen.getByText("0:01 paused")).toBeInTheDocument();
    expect(screen.getByLabelText("Microphone input level").children).toHaveLength(12);

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop & save" }));
    expect(mocks.pauseRecording).toHaveBeenCalledOnce();
    expect(mocks.discardRecording).toHaveBeenCalledOnce();
    expect(mocks.stopRecording).toHaveBeenCalledOnce();

    const toggle = screen.getByRole("button", { name: "Collapse recorder" });
    fireEvent.click(toggle);

    expect(screen.getByRole("button", { name: "Expand recorder" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("2026-08-11-153045-record.m4a")).not.toBeInTheDocument();
    expect(screen.getByText("Recording · 0:05")).toBeInTheDocument();
    expect(screen.getByLabelText("Microphone input level")).toBeInTheDocument();
  });

  it("freezes recorded time, tracks paused time, and offers resume or completion", () => {
    mocks.state = {
      ...mocks.state,
      status: "paused",
      activeSegmentStartedAt: null,
      activePauseStartedAt: 2_000,
      recordedDurationMs: 8_000,
      pausedDurationMs: 1_000,
      segmentCount: 1
    };
    const { container } = render(<RecordingDock />);

    expect(screen.getByRole("status")).toHaveTextContent("Audio recording paused");
    expect(screen.getByText("Paused · 0:08")).toBeInTheDocument();
    expect(screen.getByText("0:03 paused")).toBeInTheDocument();
    const pausedSignal = screen.getByLabelText("Microphone input paused");
    expect(pausedSignal).toHaveClass("opacity-40");
    expect(Array.from(pausedSignal.children).every((bar) => (
      (bar as HTMLElement).style.height === "3px"
    ))).toBe(true);

    vi.mocked(performance.now).mockReturnValue(14_000);
    act(() => vi.advanceTimersByTime(1_000));

    expect(screen.getByText("Paused · 0:08")).toBeInTheDocument();
    expect(screen.getByText("0:13 paused")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop & save" }));
    expect(mocks.resumeRecording).toHaveBeenCalledOnce();
    expect(mocks.discardRecording).toHaveBeenCalledOnce();
    expect(mocks.stopRecording).toHaveBeenCalledOnce();
    expect(container.firstElementChild).toHaveClass("pointer-events-auto");
    expect(container.firstElementChild).not.toHaveClass("fixed");
  });

  it("leaves no recorder panel when capture has moved to uploads", () => {
    mocks.state = {
      status: "idle",
      startedAt: null,
      activeSegmentStartedAt: null,
      activePauseStartedAt: null,
      recordedDurationMs: 0,
      pausedDurationMs: 0,
      segmentCount: 0,
      filename: null,
      destinationPath: null
    };

    const { container } = render(<RecordingDock />);

    expect(container).toBeEmptyDOMElement();
  });
});
