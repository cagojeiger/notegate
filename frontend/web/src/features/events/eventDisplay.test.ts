import { describe, expect, it } from "vitest";

import type { AuditEvent, FileChangeEvent } from "../../api/types";
import {
  formatActor,
  formatAuditAction,
  formatAuditDetail,
  formatAuditTarget,
  formatEventTimeCompact,
  formatFileChangeAction,
  formatFileChangeDetails,
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

    expect(formatAuditAction(event)).toBe("Session ended");
    expect(formatAuditDetail(event)).toBe("refresh failed");
    expect(formatAuditTarget(event)).toBe("Browser session");
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

    expect(formatFileChangeTarget(event)).toBe("Text · notes.md");
    expect(formatFileChangeAction({ ...event, op_type: "text.edit" })).toBe("Edited");
    expect(formatFileChangeAction({ ...event, op_type: "item.update", metadata: { name_changed: true } })).toBe("Renamed");
    expect(
      formatActor({ id: "user-1", kind: "user", display_name: "Ada" }, "user-1")
    ).toBe("Ada (User)");
  });

  it("shortens ids without hiding small values", () => {
    expect(shortId("short")).toBe("short");
    expect(shortId("12345678-1234-1234-1234-123456789abc")).toBe("12345678…9abc");
  });

  it("formats a compact time for narrow history rows", () => {
    expect(formatEventTimeCompact("invalid")).toBe("invalid");
    expect(formatEventTimeCompact("2026-07-13T00:05:00Z")).toMatch(/^\d{2}:\d{2}$/);
  });

  it("formats create and edit metadata as readable details", () => {
    const event = {
      id: 1,
      created_at: "2026-07-13T00:00:00Z",
      space_id: "space-1",
      node_id: "12345678-1234-1234-1234-123456789012",
      actor_account_id: "user-1",
      op_type: "file.create",
      metadata: {
        item_kind: "file",
        item_name: "archive.zip",
        parent_node_id: "87654321-4321-4321-4321-210987654321",
        byte_len_after: 1536
      }
    } satisfies FileChangeEvent;

    expect(formatFileChangeDetails(event)).toEqual([
      { label: "Parent", value: "87654321…4321" },
      { label: "Size", value: "1.5 KB" },
      { label: "Node", value: "12345678…9012" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "text.write",
      metadata: {
        byte_len_before: 1024,
        byte_len_after: 2048,
        line_count_before: 12,
        line_count_after: 18
      }
    })).toEqual([
      { label: "Size", value: "1 KB → 2 KB" },
      { label: "Lines", value: "12 → 18" },
      { label: "Node", value: "12345678…9012" }
    ]);
  });

  it("ignores malformed change metadata", () => {
    const event = {
      id: 1,
      created_at: "2026-07-13T00:00:00Z",
      space_id: "space-1",
      node_id: null,
      actor_account_id: "user-1",
      op_type: "item.copy",
      metadata: { copied_nodes: "three", recursive: "yes" }
    } satisfies FileChangeEvent;

    expect(formatFileChangeDetails(event)).toEqual([]);
  });

  it("formats move, copy, delete, and update metadata", () => {
    const event = {
      id: 1,
      created_at: "2026-07-13T00:00:00Z",
      space_id: "space-1",
      node_id: "node-1",
      actor_account_id: "user-1",
      op_type: "item.move",
      metadata: {}
    } satisfies FileChangeEvent;

    expect(formatFileChangeDetails({
      ...event,
      metadata: {
        parent_node_id_before: "parent-1",
        parent_node_id_after: "parent-2",
        name_changed: true
      }
    })).toEqual([
      { label: "From parent", value: "parent-1" },
      { label: "To parent", value: "parent-2" },
      { label: "Also renamed", value: "Yes" },
      { label: "Node", value: "node-1" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "item.copy",
      metadata: {
        copied_from_node_id: "source-1",
        parent_node_id_after: "parent-2",
        copied_nodes: 4,
        copied_texts: 2,
        copied_files: 1,
        recursive: true
      }
    })).toEqual([
      { label: "Source", value: "source-1" },
      { label: "To parent", value: "parent-2" },
      { label: "Copied items", value: "4" },
      { label: "Copied texts", value: "2" },
      { label: "Copied files", value: "1" },
      { label: "Recursive", value: "Yes" },
      { label: "Node", value: "node-1" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "item.delete",
      metadata: { deleted_nodes: 4, recursive: true }
    })).toEqual([
      { label: "Deleted items", value: "4" },
      { label: "Recursive", value: "Yes" },
      { label: "Node", value: "node-1" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "item.update",
      metadata: { name_changed: true, sort_order_changed: true }
    })).toEqual([
      { label: "Changed", value: "Name, Order" },
      { label: "Node", value: "node-1" }
    ]);
  });
});
