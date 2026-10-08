import { describe, expect, it } from "vitest";

import type { AuditEvent, FileChangeEvent } from "../../api/types";
import {
  formatActor,
  formatAuditAction,
  formatAuditDetail,
  formatAuditTarget,
  formatDuration,
  formatDurationBetween,
  formatEventTimeCompact,
  formatFileChangeAction,
  formatFileChangeDetails,
  formatFileChangeTarget,
  shortId
} from "./eventDisplay";

describe("eventDisplay", () => {
  it.each([
    ["deleted", "Body deleted", "Before deletion reason", "Checkpoint retention expired"],
    ["unavailable", "Body unavailable", "Before availability", "No body or deletion record found"]
  ])("distinguishes %s version bodies from the evidence available", (status, bodyLabel, label, value) => {
    const event: FileChangeEvent = {
      id: 1, created_at: "2026-07-13T00:00:00Z", space_id: "space", node_id: null,
      actor_account_id: null, op_type: "text.write",
      metadata: {
        before_revision_id: "revision", before_revision_status: status,
        before_revision_deletion_reason: "checkpoint_expired"
      }
    };
    expect(formatFileChangeDetails(event)).toEqual([
      { label: "Before version", value: `revision · ${bodyLabel}` },
      { label, value }
    ]);
  });

  it("keeps existing bodies visible when a deletion receipt contradicts them", () => {
    const event: FileChangeEvent = {
      id: 1, created_at: "2026-07-13T00:00:00Z", space_id: "space", node_id: null,
      actor_account_id: null, op_type: "text.write",
      metadata: {
        after_revision_id: "revision", after_revision_status: "current",
        after_revision_deletion_conflict: true, after_revision_deletion_reason: "unknown",
        after_revision_deleted_at: "2026-07-13T00:00:00Z"
      }
    };
    const details = formatFileChangeDetails(event);
    expect(details).toContainEqual({ label: "After version", value: "revision · Current body" });
    expect(details).toContainEqual({ label: "After status", value: "Deletion record conflicts with existing body" });
    expect(details).toContainEqual({ label: "After recorded deletion", value: expect.any(String) });
    expect(details.some(detail => detail.label === "After deletion reason")).toBe(false);
  });

  it("does not reinterpret future availability states or deletion reasons", () => {
    const event: FileChangeEvent = {
      id: 1, created_at: "2026-07-13T00:00:00Z", space_id: "space", node_id: null,
      actor_account_id: null, op_type: "text.write",
      metadata: { before_revision_id: "revision", before_revision_status: "new-state" }
    };
    expect(formatFileChangeDetails(event)).toEqual([{ label: "Before version", value: "revision · Availability unknown" }]);
    expect(formatFileChangeDetails({
      ...event, metadata: { ...event.metadata, before_revision_status: "deleted", before_revision_deletion_reason: "unknown" }
    })).toContainEqual({ label: "Before deletion reason", value: "Reason not recorded" });
  });

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

    expect(formatFileChangeTarget(event)).toBe("Document · notes.md");
    expect(formatFileChangeAction({ ...event, op_type: "text.edit" })).toBe("Edited");
    expect(formatFileChangeAction({ ...event, op_type: "item.update", metadata: { name_changed: true } })).toBe("Renamed");
    expect(formatFileChangeAction({
      ...event,
      op_type: "item.update",
      metadata: { write_lock_changed: true, write_locked: true }
    })).toBe("Locked");
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

  it("formats elapsed durations at readable unit boundaries", () => {
    expect(formatDuration(39.4)).toBe("39 ms");
    expect(formatDuration(999.6)).toBe("1 s");
    expect(formatDuration(1_250)).toBe("1.3 s");
    expect(formatDuration(18_400)).toBe("18 s");
    expect(formatDuration(59_600)).toBe("1m");
    expect(formatDuration(134_000)).toBe("2m 14s");
    expect(formatDuration(3_580_000)).toBe("59m 40s");
    expect(formatDuration(4_080_000)).toBe("1h 8m");
  });

  it("derives durations only from valid ordered timestamps", () => {
    expect(formatDurationBetween("2026-07-10T02:12:00.000Z", "2026-07-10T02:12:00.039Z")).toBe("39 ms");
    expect(formatDurationBetween("invalid", "2026-07-10T02:12:00.039Z")).toBeNull();
    expect(formatDurationBetween("2026-07-10T02:12:01Z", "2026-07-10T02:12:00Z")).toBeNull();
  });

  it("formats create and content-change event details", () => {
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
      { label: "File", value: "12345678…9012" }
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
      { label: "Item", value: "12345678…9012" }
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
      { label: "Item", value: "node-1" }
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
      { label: "Copied documents", value: "2" },
      { label: "Copied files", value: "1" },
      { label: "Recursive", value: "Yes" },
      { label: "Item", value: "node-1" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "item.delete",
      metadata: { deleted_nodes: 4, recursive: true }
    })).toEqual([
      { label: "Deleted items", value: "4" },
      { label: "Recursive", value: "Yes" },
      { label: "Item", value: "node-1" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "item.update",
      metadata: { name_changed: true, sort_order_changed: true }
    })).toEqual([
      { label: "Changed", value: "Name, Order" },
      { label: "Item", value: "node-1" }
    ]);
    expect(formatFileChangeDetails({
      ...event,
      op_type: "item.update",
      metadata: { write_lock_changed: true, write_locked: false }
    })).toEqual([
      { label: "Changed", value: "Write lock" },
      { label: "Item", value: "node-1" }
    ]);
  });
});
