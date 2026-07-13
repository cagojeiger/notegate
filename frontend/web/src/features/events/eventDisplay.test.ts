import { describe, expect, it } from "vitest";

import type { AuditEvent, FileChangeEvent } from "../../api/types";
import {
  formatActor,
  formatAuditAction,
  formatAuditDetail,
  formatFileChangeTarget,
  shortId
} from "./eventDisplay";

describe("eventDisplay", () => {
  it("formats audit events for people instead of exposing raw operation names", () => {
    const event = {
      id: 1,
      created_at: "2026-07-13T00:00:00Z",
      actor_account_id: "user-1",
      source: "rest",
      op_type: "session.revoke",
      resource_type: "browser_session",
      resource_id: "12345678-1234-1234-1234-123456789012",
      metadata: { reason: "refresh_failed" }
    } satisfies AuditEvent;

    expect(formatAuditAction(event)).toBe("Session access ended");
    expect(formatAuditDetail(event)).toBe("refresh failed");
  });

  it("uses content names and actor display names when available", () => {
    const event = {
      id: 1,
      created_at: "2026-07-13T00:00:00Z",
      space_id: "space-1",
      node_id: "12345678-1234-1234-1234-123456789012",
      actor_account_id: "user-1",
      op_type: "text.write",
      metadata: { item_kind: "text", item_name: "notes.md" }
    } satisfies FileChangeEvent;

    expect(formatFileChangeTarget(event)).toBe("text notes.md");
    expect(
      formatActor({ id: "user-1", kind: "user", display_name: "Ada" }, "user-1")
    ).toBe("Ada (User)");
  });

  it("shortens ids without hiding small values", () => {
    expect(shortId("short")).toBe("short");
    expect(shortId("12345678-1234-1234-1234-123456789abc")).toBe("12345678…9abc");
  });
});
