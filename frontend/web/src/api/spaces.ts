import type { ApiClient } from "./client";
import type { Space, SpacesListResponse } from "./types";

export function listSpaces(client: ApiClient): Promise<SpacesListResponse> {
  return client.get<SpacesListResponse>("/api/v1/spaces?limit=100");
}

export function createSpace(client: ApiClient, name: string): Promise<Space> {
  return client.post<Space>("/api/v1/spaces", { name });
}
