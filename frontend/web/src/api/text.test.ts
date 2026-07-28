import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { readText } from "./text";

describe("text api", () => {
  it("requests the full editable text limit", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({});

    await readText(client, "space-1", "node-1");

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/text/node-1?max_lines=5000&max_bytes=1048576");
  });
});
