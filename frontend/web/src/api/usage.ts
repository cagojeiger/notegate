import type { ApiClient } from "./client";
import { postAsyncCommand } from "./asyncCommands";
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

type SpaceUsageWire = Omit<SpaceUsage, "reconciliation"> & {
  reconciliation?: SpaceUsage["reconciliation"];
  reconciliation_pending?: boolean;
};

type CurrentUserUsageWire = Omit<CurrentUserUsage, "spaces"> & {
  spaces: SpaceUsageWire[];
};

export async function getCurrentUserUsage(client: ApiClient): Promise<CurrentUserUsage> {
  const usage = await client.get<CurrentUserUsageWire>("/api/v1/me/usage");
  return {
    ...usage,
    spaces: usage.spaces.map((space) => {
      const pending = space.reconciliation_pending === true;
      const reconciliation = space.reconciliation ?? {
        status: pending ? "pending" as const : "idle" as const,
        availability: pending
          ? { can_trigger: false, reason: "pending" as const, retry_at: null }
          : { can_trigger: true, reason: null, retry_at: null }
      };
      return { ...space, reconciliation };
    })
  };
}

export async function requestSpaceUsageCheck(
  client: ApiClient,
  spaceId: string
): Promise<AsyncCommandAck> {
  return postAsyncCommand(
    client,
    `/api/v1/spaces/${spaceId}/actions/reconcile-usage`,
    `/api/v1/spaces/${spaceId}/usage/reconcile`
  );
}
