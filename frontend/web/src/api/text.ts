import type { ApiClient } from "./client";
import type { ReadTextResponse, TextResponse } from "./types";

export function readText(client: ApiClient, spaceId: string, nodeId: string): Promise<ReadTextResponse> {
  return client.get<ReadTextResponse>(`/api/v1/spaces/${spaceId}/text/${nodeId}?max_lines=1000&max_bytes=262144`);
}

export function replaceText(client: ApiClient, spaceId: string, nodeId: string, content: string, expectedSha256?: string): Promise<TextResponse> {
  return client.put<TextResponse>(`/api/v1/spaces/${spaceId}/text/${nodeId}`, {
    storage_format: "plain",
    content,
    expected_sha256: expectedSha256
  });
}
