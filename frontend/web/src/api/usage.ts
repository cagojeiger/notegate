import type { ApiClient } from "./client";
import type { AsyncCommandAck, CommandAvailability } from "./types";

export type QuotaUsage = {
  used: number;
  limit: number;
};

export type SpaceUsage = {
  id: string;
  name: string;
  items: QuotaUsage;
  text_bytes: QuotaUsage;
  file_bytes: QuotaUsage;
  reconciliation: {
    status: "idle" | "pending";
    availability: CommandAvailability;
  };
};

export type CurrentUserUsage = {
  tier: string;
  spaces: SpaceUsage[];
};

export function getCurrentUserUsage(client: ApiClient): Promise<CurrentUserUsage> {
  return client.get<CurrentUserUsage>("/api/v1/me/usage");
}

export function requestSpaceUsageCheck(client: ApiClient, spaceId: string): Promise<AsyncCommandAck> {
  return client.post<AsyncCommandAck>(`/api/v1/spaces/${spaceId}/actions/reconcile-usage`);
}
