import type { Page } from "./types";

export type ApiKeyMetadata = {
  id: string;
  account_id: string;
  name: string;
  scopes: string[];
  expires_at: string;
  created_at: string;
  revoked_at: string | null;
};

export type MintedKey = { id: string; name: string; token: string; expires_at: string; created_at: string };

export type ApiKeyListResponse = {
  keys: ApiKeyMetadata[];
  page: Page;
};
