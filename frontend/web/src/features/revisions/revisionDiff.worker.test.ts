import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const worker = { onmessage: null as ((event: MessageEvent) => void) | null, postMessage: vi.fn() };

beforeEach(async () => {
  vi.resetModules();
  worker.postMessage.mockClear();
  worker.onmessage = null;
  vi.stubGlobal("self", worker);
  await import("./revisionDiff.worker");
});
afterEach(() => vi.unstubAllGlobals());

describe("dedicated diff Worker protocol", () => {
  it("accepts the implicit parent-port event and compares its string bodies", () => {
    worker.onmessage!(new MessageEvent("message", { data: { before: "old", after: "new" } }));
    expect(worker.postMessage).toHaveBeenCalledWith({ status: "ready", rows: [
      { kind: "change", before: { number: 1, text: "old" }, after: { number: 1, text: "new" } }
    ] });
  });
  it("rejects Window-style origins and message sources", () => {
    const data = { before: "old", after: "new" };
    worker.onmessage!(new MessageEvent("message", { origin: "https://example.com", data }));
    worker.onmessage!(new MessageEvent("message", { source: window, data }));
    expect(worker.postMessage).not.toHaveBeenCalled();
  });
  it("rejects malformed payloads before performing work", () => {
    for (const data of [null, "text", {}, { before: 1, after: "new" }, { before: "old", after: null }]) {
      worker.onmessage!(new MessageEvent("message", { data }));
    }
    expect(worker.postMessage).not.toHaveBeenCalled();
  });
});
