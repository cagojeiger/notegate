import type { Space } from "../../src/api/types";
import type { CurrentUserUsage } from "../../src/api/usage";

export function usageResponse(
  space: Pick<Space, "id" | "name">,
  itemsUsed = 1
): CurrentUserUsage {
  return {
    tier: "max",
    spaces: [{
      id: space.id,
      name: space.name,
      items: { used: itemsUsed, limit: 2_000 },
      text_bytes: { used: 0, limit: 1024 * 1024 },
      file_bytes: { used: 0, limit: 1024 * 1024 * 1024 },
      reconciliation: {
        status: "idle",
        availability: { can_trigger: true, reason: null, retry_at: null }
      }
    }]
  };
}
