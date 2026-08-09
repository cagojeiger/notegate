import { describe, expect, it } from "vitest";

import { POLLING } from "../../api/polling";
import { linkIndexPollInterval } from "./useLinkIndexQueries";

describe("linkIndexPollInterval", () => {
  it.each(["pending", "syncing", "retrying"] as const)("polls while %s", (status) => {
    expect(linkIndexPollInterval(status)).toBe(POLLING.linkIndexPendingMs);
  });

  it.each([undefined, "up_to_date", "failed"] as const)("stops while %s", (status) => {
    expect(linkIndexPollInterval(status)).toBe(false);
  });
});
