import { describe, expect, it } from "vitest";

import { createMockApiClient } from "../test/apiClient";
import { readText, replaceText, updateTextEncryption } from "./text";

describe("text api", () => {
  it("requests the full editable text limit", async () => {
    const client = createMockApiClient();
    client.get.mockResolvedValue({});

    await readText(client, "space-1", "node-1");

    expect(client.get).toHaveBeenCalledWith("/api/v1/spaces/space-1/text/node-1?max_lines=5000&max_bytes=1048576");
  });

  it("sends the latest revision with text mutations", async () => {
    const client = createMockApiClient();
    client.put.mockResolvedValue({});

    await replaceText(client, "space-1", "node-1", "updated", 9, "sha-1");
    await updateTextEncryption(client, "space-1", "node-1", true, 9);

    expect(client.put).toHaveBeenNthCalledWith(
      1,
      "/api/v1/spaces/space-1/text/node-1",
      {
        storage_format: "plain",
        content: "updated",
        expected_revision: 9,
        expected_sha256: "sha-1"
      }
    );
    expect(client.put).toHaveBeenNthCalledWith(
      2,
      "/api/v1/spaces/space-1/text/node-1/encryption",
      { enabled: true, expected_revision: 9 }
    );
  });
});
