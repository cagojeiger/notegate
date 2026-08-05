import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { replaceMetadata } from "./metadata";

describe("metadata api", () => {
  it("sends the latest revision with metadata replacement", async () => {
    const client = createMockApiClient();
    client.put.mockResolvedValue({});

    await replaceMetadata(client, "space-1", "node-1", { owner: "docs" }, 4);

    expect(client.put).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/nodes/node-1/metadata",
      { metadata: { owner: "docs" }, expected_revision: 4 }
    );
  });
});
