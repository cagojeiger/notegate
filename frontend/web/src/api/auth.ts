import type { ApiClient } from "./client";

export function logout(client: ApiClient): Promise<void> {
  return client.post<void>("/auth/logout");
}
