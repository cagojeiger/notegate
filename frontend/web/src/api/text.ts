import type { ApiClient } from "./client";
import type { ReadTextResponse, RestNode, TextResponse } from "./types";

const TEXT_READ_MAX_LINES = 5_000;
const TEXT_READ_MAX_BYTES = 1_048_576;
const TEXT_CONDITIONAL_MIN_BYTES = 1_024;

export async function readText(client: ApiClient, spaceId: string, nodeId: string, previous?: ReadTextResponse): Promise<ReadTextResponse> {
  const path = `/api/v1/spaces/${spaceId}/text/${nodeId}?max_lines=${TEXT_READ_MAX_LINES}&max_bytes=${TEXT_READ_MAX_BYTES}`;
  const cached = previous?.text;
  const reusable = previous?.node.id === nodeId && cached?.node_id === nodeId
    && cached.storage_format === "plain" && "content" in cached && cached.content.length > 0
    && cached.content_sha256 && cached.byte_len >= TEXT_CONDITIONAL_MIN_BYTES && cached.byte_len <= TEXT_READ_MAX_BYTES
    && cached.line_count > 0 && cached.line_count <= TEXT_READ_MAX_LINES
    && cached.start_line === 1 && cached.end_line === cached.line_count && cached.returned_lines === cached.line_count
    && cached.truncated === false && cached.next_start_line === null ? cached : undefined;
  const response = await client.get<ReadTextResponse>(reusable
    ? `${path}&if_none_match_sha256=${encodeURIComponent(reusable.content_sha256)}`
    : path);
  if (!("unchanged" in response.text)) return response;

  const text = response.text;
  if (reusable && text.unchanged === true && text.content_returned === false && response.node.id === nodeId
    && text.node_id === nodeId && text.storage_format === "plain" && text.content_sha256 === reusable.content_sha256
    && text.updated_at && text.updated_by) {
    return {
      node: response.node,
      text: { ...reusable, byte_len: text.byte_len, line_count: text.line_count,
        updated_at: text.updated_at, updated_by: text.updated_by }
    };
  }

  // Older servers omit metadata; a changed representation also requires a full read.
  const full = reusable ? await client.get<ReadTextResponse>(path) : response;
  if ("unchanged" in full.text) throw new Error("Text read response omitted the document content");
  return full;
}

export function replaceText(client: ApiClient, spaceId: string, nodeId: string, content: string, expectedSha256?: string, editSessionId?: string): Promise<TextResponse> {
  return client.put<TextResponse>(`/api/v1/spaces/${spaceId}/text/${nodeId}`, {
    storage_format: "plain",
    content,
    expected_sha256: expectedSha256,
    edit_session_id: editSessionId
  });
}

export function updateTextEncryption(
  client: ApiClient,
  spaceId: string,
  nodeId: string,
  enabled: boolean
): Promise<RestNode> {
  return client.put<RestNode>(
    `/api/v1/spaces/${spaceId}/text/${nodeId}/encryption`,
    { enabled }
  );
}
