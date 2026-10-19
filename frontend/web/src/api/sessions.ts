import type { ApiClient } from "./client";

export type BrowserSession = {
  id: string;
  current: boolean;
  client_environment: string | null;
  created_at: string;
  last_used_at: string | null;
  idle_expires_at: string;
  expires_at: string;
};

export type OAuthGrant = {
  id: string;
  client_name: string;
  kind: "cli" | "mcp";
  client_environment: string | null;
  created_at: string;
  last_token_at: string | null;
  idle_expires_at: string;
  expires_at: string;
  offline_access: boolean;
};

export function listBrowserSessions(client: ApiClient): Promise<{ sessions: BrowserSession[] }> {
  return client.get("/api/v1/me/sessions");
}

export function listOAuthGrants(client: ApiClient): Promise<{ grants: OAuthGrant[] }> {
  return client.get("/api/v1/me/oauth-grants");
}
