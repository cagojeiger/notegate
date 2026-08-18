import { beforeEach, describe, expect, it, vi } from "vitest";

import { finalizeRecordedAudio } from "./finalizeRecordedAudio";

const ebmlMocks = vi.hoisted(() => ({
  cues: [{ CueTrack: 1, CueClusterPosition: 4, CueTime: 0 }],
  decode: vi.fn(),
  makeMetadataSeekable: vi.fn(),
  read: vi.fn(),
  stop: vi.fn()
}));

vi.mock("./ebmlBrowser", () => ({
  loadEbml: async () => ({
    Decoder: class MockDecoder {
      decode = ebmlMocks.decode;
    },
    Reader: class MockReader {
      cues = ebmlMocks.cues;
      drop_default_duration = true;
      duration = 1_000;
      logging = true;
      metadataSize = 2;
      metadatas = [{ name: "EBML" }];
      read = ebmlMocks.read;
      stop = ebmlMocks.stop;
    },
    tools: { makeMetadataSeekable: ebmlMocks.makeMetadataSeekable }
  })
}));

describe("finalizeRecordedAudio", () => {
  beforeEach(() => {
    ebmlMocks.cues = [{ CueTrack: 1, CueClusterPosition: 4, CueTime: 0 }];
    ebmlMocks.decode.mockReset().mockReturnValue([{ name: "Cluster" }]);
    ebmlMocks.read.mockReset();
    ebmlMocks.stop.mockReset();
    ebmlMocks.makeMetadataSeekable.mockReset().mockReturnValue(new Uint8Array([9, 8, 7]));
  });

  it("replaces MediaRecorder metadata while preserving the recorded body", async () => {
    const original = streamingBlob(
      new Uint8Array([1, 2, 3, 4]),
      "audio/webm;codecs=opus"
    );

    const finalized = await finalizeRecordedAudio(original);

    expect(ebmlMocks.decode).toHaveBeenCalled();
    expect(ebmlMocks.read).toHaveBeenCalledWith({ name: "Cluster" });
    expect(ebmlMocks.stop).toHaveBeenCalledOnce();
    expect(ebmlMocks.makeMetadataSeekable).toHaveBeenCalledWith(
      [{ name: "EBML" }],
      1_000,
      [{ CueTrack: 1, CueClusterPosition: 4, CueTime: 0 }]
    );
    expect(Array.from(await blobBytes(finalized))).toEqual([9, 8, 7, 3, 4]);
    expect(finalized.type).toBe(original.type);
  });

  it.each([
    [new Blob(["original"], { type: "audio/mp4" })],
    [new Blob([], { type: "audio/webm" })]
  ])("leaves unsupported or empty recordings unchanged", async (original) => {
    await expect(finalizeRecordedAudio(original)).resolves.toBe(original);
    expect(ebmlMocks.decode).not.toHaveBeenCalled();
  });

  it("leaves a WebM unchanged when it has no usable cue metadata", async () => {
    ebmlMocks.cues = [];
    const original = streamingBlob(new Uint8Array([1, 2, 3]), "audio/webm");

    await expect(finalizeRecordedAudio(original)).resolves.toBe(original);
    expect(ebmlMocks.makeMetadataSeekable).not.toHaveBeenCalled();
  });
});

function streamingBlob(bytes: Uint8Array<ArrayBuffer>, type: string) {
  const blob = new Blob([bytes], { type });
  Object.defineProperty(blob, "stream", {
    value: () => {
      let delivered = false;
      return {
        getReader: () => ({
          read: async () => {
            if (delivered) return { done: true, value: undefined };
            delivered = true;
            return { done: false, value: bytes };
          }
        })
      };
    }
  });
  return blob;
}

function blobBytes(blob: Blob) {
  return new Promise<Uint8Array>((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(new Uint8Array(reader.result as ArrayBuffer)));
    reader.addEventListener("error", () => reject(reader.error));
    reader.readAsArrayBuffer(blob);
  });
}
