import type { ApiClient } from "./client";
import type { ReadTextResponse, TextResponse } from "./types";

const TEXT_READ_MAX_LINES = 5_000;
const TEXT_READ_MAX_BYTES = 1_048_576;

export function readText(client: ApiClient, spaceId: string, nodeId: string): Promise<ReadTextResponse> {
  return client.get<ReadTextResponse>(`/api/v1/spaces/${spaceId}/text/${nodeId}?max_lines=${TEXT_READ_MAX_LINES}&max_bytes=${TEXT_READ_MAX_BYTES}`);
}

export function replaceText(client: ApiClient, spaceId: string, nodeId: string, content: string, expectedSha256?: string): Promise<TextResponse> {
  return client.put<TextResponse>(`/api/v1/spaces/${spaceId}/text/${nodeId}`, {
    storage_format: "plain",
    content,
    expected_sha256: expectedSha256
  });
}
