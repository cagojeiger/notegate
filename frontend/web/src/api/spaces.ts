import type { ApiClient } from "./client";
import type { Space, SpacesListResponse } from "./types";

export type UpdateSpaceInput = {
  name?: string;
  sort_order?: number;
  navigation_pinned?: boolean;
  user_mcp_enabled?: boolean;
  default_search_enabled?: boolean;
  default_text_encryption_enabled?: boolean;
};

export function listSpaces(client: ApiClient): Promise<SpacesListResponse> {
  return client.get<SpacesListResponse>("/api/v1/spaces?limit=100");
}

export function createSpace(client: ApiClient, name: string): Promise<Space> {
  return client.post<Space>("/api/v1/spaces", { name });
}

export function updateSpace(client: ApiClient, spaceId: string, input: UpdateSpaceInput): Promise<Space> {
  return client.patch<Space>(`/api/v1/spaces/${spaceId}`, input);
}

export function reorderSpaces(client: ApiClient, updates: Array<{ spaceId: string; sort_order: number }>): Promise<void> {
  return client.post<void>("/api/v1/spaces:reorder", {
    updates: updates.map(({ spaceId, sort_order }) => ({ space_id: spaceId, sort_order }))
  });
}

export function deleteSpace(client: ApiClient, spaceId: string): Promise<void> {
  return client.delete<void>(`/api/v1/spaces/${spaceId}`);
}
