import { describe, expect, it } from "vitest";

import { formatMetadata, shortId } from "./eventDisplay";

describe("eventDisplay", () => {
  it("formats metadata as compact key-value text", () => {
    expect(formatMetadata({ changed_fields: ["name"], recursive: false })).toBe('changed_fields=["name"] · recursive=false');
  });

  it("shortens ids without hiding small values", () => {
    expect(shortId("short")).toBe("short");
    expect(shortId("12345678-1234-1234-1234-123456789abc")).toBe("12345678…9abc");
  });
});
