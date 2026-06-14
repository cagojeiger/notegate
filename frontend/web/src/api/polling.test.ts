import { afterEach, describe, expect, it, vi } from "vitest";

import { withPollingJitter } from "./polling";

describe("withPollingJitter", () => {
  afterEach(() => vi.restoreAllMocks());

  it("keeps intervals inside the configured jitter window", () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    expect(withPollingJitter(30_000, 5_000)).toBe(25_000);

    vi.spyOn(Math, "random").mockReturnValue(0.5);
    expect(withPollingJitter(30_000, 5_000)).toBe(30_000);

    vi.spyOn(Math, "random").mockReturnValue(1);
    expect(withPollingJitter(30_000, 5_000)).toBe(35_000);
  });

  it("keeps a minimum practical interval", () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    expect(withPollingJitter(500, 5_000)).toBe(1_000);
  });
});
