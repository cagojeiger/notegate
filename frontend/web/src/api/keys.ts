import type { ApiClient } from "./client";

export type ApiKeyMetadata = {
  id: string;
  account_id: string;
  name: string;
  scopes: string[];
  expires_at: string;
  created_at: string;
  revoked_at: string | null;
};

// Create/rotate return the plaintext token exactly once.
export type CreatedApiKey = ApiKeyMetadata & { token: string };

export type ApiKeyListResponse = {
  keys: ApiKeyMetadata[];
  page: { limit: number; returned: number; has_more: boolean; next_cursor: string | null };
};

export function listMyKeys(client: ApiClient): Promise<ApiKeyListResponse> {
  return client.get<ApiKeyListResponse>("/api/v1/me/keys?limit=100");
}

export function createMyKey(client: ApiClient, input: { name: string; expires_at: string }): Promise<CreatedApiKey> {
  return client.post<CreatedApiKey>("/api/v1/me/keys", { name: input.name, expires_at: input.expires_at });
}

export function rotateMyKey(client: ApiClient, keyId: string): Promise<CreatedApiKey> {
  return client.post<CreatedApiKey>(`/api/v1/me/keys/${keyId}`);
}

export function revokeMyKey(client: ApiClient, keyId: string): Promise<void> {
  return client.delete<void>(`/api/v1/me/keys/${keyId}`);
}
