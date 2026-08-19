import { StrictMode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AudioPreview } from "./AudioPreview";

const finalizerMocks = vi.hoisted(() => ({
  finalizeRecordedAudio: vi.fn<(blob: Blob) => Promise<Blob>>()
}));

vi.mock("../recording/finalizeRecordedAudio", () => finalizerMocks);

describe("AudioPreview", () => {
  beforeEach(() => {
    finalizerMocks.finalizeRecordedAudio.mockReset().mockImplementation(async (blob) => (
      new Blob([blob, "seek metadata"], { type: blob.type })
    ));
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(audioResponse()));
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:repaired-audio");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  });

  it("keeps normal audio on its streaming URL", () => {
    renderPreview({ mediaType: "audio/mp4" });

    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, 120);
    fireEvent.loadedMetadata(audio);

    expect(audio).toHaveAttribute("src", "https://storage.example/meeting.webm");
    expect(fetch).not.toHaveBeenCalled();
    expect(finalizerMocks.finalizeRecordedAudio).not.toHaveBeenCalled();
  });

  it("repairs a bounded legacy WebM when its duration is not finite", async () => {
    renderPreview();

    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);

    expect(screen.getByRole("status")).toHaveTextContent("Preparing older recording…");
    await waitFor(() => expect(audio).toHaveAttribute("src", "blob:repaired-audio"));
    expect(fetch).toHaveBeenCalledWith(
      "https://storage.example/meeting.webm",
      expect.objectContaining({
        cache: "no-store",
        credentials: "omit",
        referrerPolicy: "no-referrer",
        signal: expect.any(AbortSignal)
      })
    );
    expect(finalizerMocks.finalizeRecordedAudio).toHaveBeenCalledWith(
      expect.objectContaining({ type: "audio/webm" })
    );
    expect(URL.createObjectURL).toHaveBeenCalledTimes(1);
  });

  it("repairs after the development StrictMode effect cycle", async () => {
    render(
      <StrictMode>
        <AudioPreview
          url="https://storage.example/meeting.webm"
          name="meeting.webm"
          mediaType="audio/webm"
          byteLen={21_653_472}
          onError={vi.fn()}
        />
      </StrictMode>
    );

    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);

    await waitFor(() => expect(audio).toHaveAttribute("src", "blob:repaired-audio"));
  });

  it.each([
    ["non-WebM audio", { mediaType: "audio/mp4" }],
    ["audio without a known size", { byteLen: undefined }],
    ["oversized WebM audio", { byteLen: 64 * 1024 * 1024 + 1 }]
  ])("does not buffer %s", (_label, overrides) => {
    renderPreview(overrides);
    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);

    expect(fetch).not.toHaveBeenCalled();
  });

  it("reports a playback error after a repair request fails", async () => {
    const onError = vi.fn();
    vi.mocked(fetch).mockResolvedValue(audioResponse({ ok: false }));
    renderPreview({ onError });

    fireEvent.error(screen.getByLabelText("Play meeting.webm"));

    await waitFor(() => expect(onError).toHaveBeenCalledTimes(1));
    expect(screen.getByText(/could not be prepared/)).toBeInTheDocument();
  });

  it("reports a repaired source failure only once", async () => {
    const onError = vi.fn();
    renderPreview({ onError });
    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);
    await waitFor(() => expect(audio).toHaveAttribute("src", "blob:repaired-audio"));

    fireEvent.error(audio);
    fireEvent.error(audio);

    expect(onError).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("keeps the original stream when opportunistic repair is not possible", async () => {
    finalizerMocks.finalizeRecordedAudio.mockImplementation(async (blob) => blob);
    const onError = vi.fn();
    renderPreview({ onError });

    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);

    await screen.findByText(/could not be prepared/);
    expect(audio).toHaveAttribute("src", "https://storage.example/meeting.webm");
    expect(onError).not.toHaveBeenCalled();
  });

  it("aborts an in-flight repair without reporting a playback failure", () => {
    let repairSignal: AbortSignal | undefined;
    const onError = vi.fn();
    vi.mocked(fetch).mockImplementation((_url, init) => {
      repairSignal = init?.signal ?? undefined;
      return new Promise((_resolve, reject) => {
        repairSignal?.addEventListener("abort", () => {
          reject(new DOMException("Audio preview canceled", "AbortError"));
        }, { once: true });
      });
    });
    const view = renderPreview({ onError });
    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);

    view.unmount();

    expect(repairSignal?.aborted).toBe(true);
    expect(onError).not.toHaveBeenCalled();
  });

  it("revokes a repaired object URL on unmount", async () => {
    let repairSignal: AbortSignal | undefined;
    vi.mocked(fetch).mockImplementation((_url, init) => {
      repairSignal = init?.signal ?? undefined;
      return Promise.resolve(audioResponse());
    });
    const view = renderPreview();
    const audio = screen.getByLabelText("Play meeting.webm");
    setDuration(audio, Number.POSITIVE_INFINITY);
    fireEvent.loadedMetadata(audio);

    await waitFor(() => expect(audio).toHaveAttribute("src", "blob:repaired-audio"));
    view.unmount();

    expect(repairSignal?.aborted).toBe(true);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:repaired-audio");
  });
});

function renderPreview(overrides: Partial<Parameters<typeof AudioPreview>[0]> = {}) {
  return render(<AudioPreview
    url="https://storage.example/meeting.webm"
    name="meeting.webm"
    mediaType="audio/webm"
    byteLen={21_653_472}
    onError={vi.fn()}
    {...overrides}
  />);
}

function setDuration(audio: HTMLElement, duration: number) {
  Object.defineProperty(audio, "duration", { configurable: true, value: duration });
}

function audioResponse(overrides: { ok?: boolean } = {}): Response {
  return {
    ok: overrides.ok ?? true,
    headers: new Headers({ "content-length": "4" }),
    blob: async () => new Blob(["webm"], { type: "audio/webm" })
  } as Response;
}
