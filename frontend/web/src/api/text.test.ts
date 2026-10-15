import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { ApiError } from "./errors";
import { readText } from "./text";
import type { ReadTextResponse } from "./types";

const path = "/api/v1/spaces/space-1/text/node-1?max_lines=5000&max_bytes=1048576";
const hash = "a".repeat(64);
const conditionalPath = `${path}&if_none_match_sha256=${hash}`;
type PlainText = Extract<ReadTextResponse["text"], { storage_format: "plain" }>;
type UnchangedText = Extract<ReadTextResponse["text"], { unchanged: boolean }>;

function fullText(overrides: Partial<PlainText> = {}): ReadTextResponse & { text: PlainText } {
  return {
    node: { id: "node-1", path: "/before.md" },
    text: {
      node_id: "node-1", storage_format: "plain", content: "a".repeat(2048), content_sha256: hash,
      byte_len: 2048, line_count: 1, start_line: 1, end_line: 1, returned_lines: 1,
      truncated: false, next_start_line: null,
      updated_at: "2026-10-01T00:00:00Z",
      updated_by: { id: "user-1", kind: "user", display_name: "First writer" },
      ...overrides
    }
  };
}

function unchanged(overrides: Partial<UnchangedText> = {}): ReadTextResponse & { text: UnchangedText } {
  return {
    node: { id: "node-1", path: "/renamed.md" },
    text: {
      node_id: "node-1", storage_format: "plain", unchanged: true, content_returned: false,
      content_sha256: hash, byte_len: 2048, line_count: 1,
      updated_at: "2026-10-02T00:00:00Z",
      updated_by: { id: "agent-1", kind: "agent", display_name: "Latest writer" },
      ...overrides
    }
  };
}

describe("text api", () => {
  it("requests the full editable text limit without a cached snapshot", async () => {
    const client = createMockApiClient();
    const response = fullText();
    client.get.mockResolvedValue(response);

    expect(await readText(client, "space-1", "node-1")).toEqual(response);
    expect(client.get).toHaveBeenCalledTimes(1);
    expect(client.get).toHaveBeenCalledWith(path);
  });

  it("reuses the complete request snapshot with the response's latest path and metadata", async () => {
    const client = createMockApiClient();
    const previous = fullText();
    const response = unchanged();
    client.get.mockResolvedValue(response);

    const result = await readText(client, "space-1", "node-1", previous);

    expect(client.get).toHaveBeenCalledTimes(1);
    expect(client.get).toHaveBeenCalledWith(conditionalPath);
    expect(result).toEqual({
      node: response.node,
      text: { ...previous.text, updated_at: response.text.updated_at, updated_by: response.text.updated_by }
    });
    expect(result.text).not.toHaveProperty("unchanged");
    expect(result.text).not.toHaveProperty("content_returned");
    expect(previous).toEqual(fullText());
  });

  it("returns a changed full body without another request", async () => {
    const client = createMockApiClient();
    const response = fullText({ content: "b".repeat(2048), content_sha256: "b".repeat(64) });
    client.get.mockResolvedValue(response);

    expect(await readText(client, "space-1", "node-1", fullText())).toEqual(response);
    expect(client.get).toHaveBeenCalledTimes(1);
    expect(client.get).toHaveBeenCalledWith(conditionalPath);
  });

  it.each([
    { name: "empty", previous: fullText({ content: "", byte_len: 0, line_count: 0, end_line: 0, returned_lines: 0 }) },
    { name: "below 1KiB", previous: fullText({ content: "a".repeat(1023), byte_len: 1023 }) },
    { name: "truncated", previous: fullText({ truncated: true }) },
    { name: "later start", previous: fullText({ start_line: 2 }) },
    { name: "missing final line", previous: fullText({ line_count: 2 }) },
    { name: "incomplete returned range", previous: fullText({ returned_lines: 0 }) },
    { name: "continuation", previous: fullText({ next_start_line: 2 }) },
    { name: "over byte limit", previous: fullText({ byte_len: 1_048_577 }) },
    { name: "over line limit", previous: fullText({ line_count: 5001, end_line: 5001, returned_lines: 5001 }) },
    { name: "different outer node", previous: { ...fullText(), node: { id: "node-2", path: "/other.md" } } },
    { name: "different text node", previous: fullText({ node_id: "node-2" }) },
    { name: "missing hash", previous: fullText({ content_sha256: "" }) },
    { name: "bare marker", previous: unchanged() },
    { name: "client ciphertext", previous: {
      node: fullText().node,
      text: { node_id: "node-1", storage_format: "encrypted", encrypted_payload: { ciphertext: "opaque" },
        content_sha256: hash, byte_len: 2048, line_count: 1,
        updated_at: "2026-10-01T00:00:00Z", updated_by: fullText().text.updated_by }
    } }
  ] satisfies Array<{ name: string; previous: ReadTextResponse }>)("uses a full read for $name snapshots", async ({ previous }) => {
    const client = createMockApiClient();
    const response = fullText();
    client.get.mockResolvedValue(response);

    expect(await readText(client, "space-1", "node-1", previous)).toEqual(response);
    expect(client.get).toHaveBeenCalledTimes(1);
    expect(client.get).toHaveBeenCalledWith(path);
  });

  it.each([
    { name: "1KiB", previous: fullText({ content: "a".repeat(1024), byte_len: 1024 }) },
    { name: "1MiB", previous: fullText({ content: "a".repeat(1_048_576), byte_len: 1_048_576 }) },
    { name: "5000 lines", previous: fullText({ content: "a\n".repeat(5000), byte_len: 10_000, line_count: 5000, end_line: 5000, returned_lines: 5000 }) }
  ])("accepts complete snapshots at the $name boundary", async ({ previous }) => {
    const client = createMockApiClient();
    client.get.mockResolvedValue(unchanged({ byte_len: previous.text.byte_len, line_count: previous.text.line_count }));

    expect(await readText(client, "space-1", "node-1", previous)).toMatchObject({ text: { content: previous.text.content } });
    expect(client.get).toHaveBeenCalledTimes(1);
    expect(client.get).toHaveBeenCalledWith(conditionalPath);
  });

  it.each([
    { name: "different hash", response: unchanged({ content_sha256: "b".repeat(64) }) },
    { name: "different outer node", response: { ...unchanged(), node: { id: "node-2", path: "/other.md" } } },
    { name: "different text node", response: unchanged({ node_id: "node-2" }) },
    { name: "changed format", response: unchanged({ storage_format: "encrypted" }) },
    { name: "old server without timestamp", response: unchanged({ updated_at: undefined }) },
    { name: "old server without author", response: unchanged({ updated_by: undefined }) },
    { name: "not unchanged", response: unchanged({ unchanged: false }) },
    { name: "claims content without returning it", response: unchanged({ content_returned: true }) }
  ])("falls back once for $name", async ({ response }) => {
    const client = createMockApiClient();
    const full = fullText();
    client.get.mockResolvedValueOnce(response).mockResolvedValueOnce(full);

    expect(await readText(client, "space-1", "node-1", fullText())).toEqual(full);
    expect(client.get.mock.calls.map(([url]) => url)).toEqual([conditionalPath, path]);
  });

  it("rejects a fallback marker instead of returning it or retrying indefinitely", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue(unchanged({ updated_at: undefined }));

    await expect(readText(client, "space-1", "node-1", fullText())).rejects.toThrow("omitted the document content");
    expect(client.get.mock.calls.map(([url]) => url)).toEqual([conditionalPath, path]);
  });

  it.each([
    { status: 403, fallback: false }, { status: 404, fallback: false },
    { status: 403, fallback: true }, { status: 404, fallback: true }
  ])("preserves HTTP $status errors (fallback: $fallback)", async ({ status, fallback }) => {
    const client = createMockApiClient();
    const error = new ApiError("Resource is not accessible", status);
    if (fallback) client.get.mockResolvedValueOnce(unchanged({ updated_at: undefined }));
    client.get.mockRejectedValueOnce(error);

    await expect(readText(client, "space-1", "node-1", fullText())).rejects.toBe(error);
    expect(client.get).toHaveBeenCalledTimes(fallback ? 2 : 1);
  });
});
