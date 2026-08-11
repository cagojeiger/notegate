import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RecordingDock } from "./RecordingDock";

const mocks = vi.hoisted(() => ({
  discardRecording: vi.fn(),
  signal: Array.from({ length: 12 }, (_, index) => index / 11),
  state: {
    status: "recording" as "idle" | "requesting" | "recording" | "stopping",
    startedAt: Date.now() as number | null,
    filename: "2026-08-11-153045-record.m4a" as string | null,
    destinationPath: "/" as string | null
  },
  stopRecording: vi.fn()
}));

vi.mock("./AudioRecordingProvider", () => ({
  useAudioRecordingActions: () => ({
    discardRecording: mocks.discardRecording,
    stopRecording: mocks.stopRecording
  }),
  useAudioRecordingSignal: () => mocks.signal,
  useAudioRecordingState: () => mocks.state
}));

describe("RecordingDock", () => {
  beforeEach(() => {
    mocks.discardRecording.mockReset();
    mocks.stopRecording.mockReset();
    mocks.state = {
      status: "recording",
      startedAt: Date.now(),
      filename: "2026-08-11-153045-record.m4a",
      destinationPath: "/"
    };
  });

  it("shows the root recording, live signal, and compact actions", () => {
    render(<RecordingDock />);

    expect(screen.getByText(/Recording · 0:00/)).toBeInTheDocument();
    expect(screen.getByText("2026-08-11-153045-record.m4a · /")).toBeInTheDocument();
    expect(screen.getByLabelText("Microphone input level").children).toHaveLength(12);

    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop & save" }));
    expect(mocks.discardRecording).toHaveBeenCalledOnce();
    expect(mocks.stopRecording).toHaveBeenCalledOnce();
  });

  it("leaves no recorder row when capture has moved to uploads", () => {
    mocks.state = {
      status: "idle",
      startedAt: null,
      filename: null,
      destinationPath: null
    };

    const { container } = render(<RecordingDock />);

    expect(container).toBeEmptyDOMElement();
  });
});
