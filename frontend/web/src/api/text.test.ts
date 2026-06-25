import { describe, expect, it, vi } from "vitest";

import type { ApiClient } from "./client";
import { readText } from "./text";

describe("text api", () => {
  it("requests the full editable text limit", async () => {
    const client = { get: vi.fn().mockResolvedValue({}) } as unknown as ApiClient;

    await readText(client, "space-1", "node-1");

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/text/node-1?max_lines=5000&max_bytes=1048576");
  });
});
