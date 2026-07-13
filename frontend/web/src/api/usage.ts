import type { ApiClient } from "./client";

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
  reconciliation_pending: boolean;
};

export type CurrentUserUsage = {
  tier: string;
  spaces: SpaceUsage[];
};

type ReconciliationQueuedResponse = {
  status: "queued";
};

export function getCurrentUserUsage(client: ApiClient): Promise<CurrentUserUsage> {
  return client.get<CurrentUserUsage>("/api/v1/me/usage");
}

export function requestSpaceUsageCheck(client: ApiClient, spaceId: string): Promise<ReconciliationQueuedResponse> {
  return client.post<ReconciliationQueuedResponse>(`/api/v1/spaces/${spaceId}/usage/reconcile`);
}
