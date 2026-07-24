import type { ApiClient } from "./client";
import type { Me } from "../entities/account/model";

export function getMe(client: ApiClient): Promise<Me> {
  return client.get<Me>("/api/v1/me");
}
