import type { ApiClient } from "./client";
import type { Me } from "./types";

export function getMe(client: ApiClient): Promise<Me> {
  return client.get<Me>("/api/v1/me");
}
