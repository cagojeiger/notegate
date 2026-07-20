import type { AccountRef, AuditEvent, FileChangeEvent } from "../../api/types";
import { formatBytes } from "../../shared/lib/formatBytes";

export type FileChangeDetail = {
  label: string;
  value: string;
};

const numberFormatter = new Intl.NumberFormat("en-US");

const FILE_CHANGE_ACTIONS: Record<string, string> = {
  "folder.create": "Created",
  "text.create": "Created",
  "file.create": "Created",
  "text.write": "Edited",
  "text.append": "Edited",
  "text.patch": "Edited",
  "text.edit": "Edited",
  "item.move": "Moved",
  "item.copy": "Copied",
  "item.delete": "Deleted",
  "metadata.replace": "Updated metadata",
  "metadata.patch": "Updated metadata"
};

const ITEM_KIND_LABELS: Record<string, string> = {
  folder: "Folder",
  text: "Text",
  file: "File"
};

const AUDIT_TARGET_LABELS: Record<string, string> = {
  account: "Account",
  agent: "Agent",
  api_key: "API key",
  browser_session: "Browser session",
  space: "Space"
};

const AUDIT_ACTIONS: Record<string, string> = {
  "account.create": "Created account",
  "account.delete": "Deleted account",
  "session.login": "Signed in",
  "session.logout": "Signed out",
  "session.revoke": "Session ended",
  "space.create": "Created a space",
  "space.update": "Updated a space",
  "space.delete": "Deleted a space",
  "agent.create": "Created an agent",
  "agent.delete": "Deleted an agent",
  "user_key.create": "Created a user API key",
  "user_key.rotate": "Rotated a user API key",
  "user_key.revoke": "Revoked a user API key",
  "agent_key.create": "Created an agent API key",
  "agent_key.rotate": "Rotated an agent API key",
  "agent_key.revoke": "Revoked an agent API key",
  "connection.upsert": "Changed agent access",
  "connection.disconnect": "Disconnected an agent"
};

export function formatEventTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  }).format(date);
}

export function formatEventTimeCompact(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false
  }).format(date);
}

export function shortId(value: string | null | undefined): string {
  if (!value) return "—";
  return value.length <= 12 ? value : `${value.slice(0, 8)}…${value.slice(-4)}`;
}

export function formatFileChangeAction(event: FileChangeEvent): string {
  if (event.op_type === "item.update") {
    const renamed = event.metadata.name_changed === true;
    const reordered = event.metadata.sort_order_changed === true;
    if (renamed && !reordered) return "Renamed";
    if (reordered && !renamed) return "Reordered";
    return "Updated";
  }
  return FILE_CHANGE_ACTIONS[event.op_type] ?? event.op_type;
}

export function formatAuditAction(event: AuditEvent): string {
  return AUDIT_ACTIONS[event.op_type] ?? event.op_type;
}

export function formatAuditTarget(event: AuditEvent): string {
  const label = AUDIT_TARGET_LABELS[event.resource_type] ?? event.resource_type.replace(/_/g, " ");
  if (event.resource_type === "browser_session") return label;
  return event.resource_id ? `${label} ${shortId(event.resource_id)}` : label;
}

export function formatAuditDetail(event: AuditEvent): string | null {
  if (typeof event.metadata.reason === "string") return event.metadata.reason.replace(/_/g, " ");
  if (typeof event.metadata.permission === "string") return `${event.metadata.permission} access`;
  if (Array.isArray(event.metadata.changed_fields)) return `Changed ${event.metadata.changed_fields.join(", ")}`;
  return null;
}

export function formatFileChangeTarget(event: FileChangeEvent): string {
  const kind = typeof event.metadata.item_kind === "string" ? event.metadata.item_kind : "item";
  const name = typeof event.metadata.item_name === "string" ? event.metadata.item_name : shortId(event.node_id);
  return `${ITEM_KIND_LABELS[kind] ?? "Item"} · ${name}`;
}

export function formatFileChangeDetails(event: FileChangeEvent): FileChangeDetail[] {
  const metadata = event.metadata;
  const details: FileChangeDetail[] = [];

  if (event.op_type === "folder.create" || event.op_type === "text.create" || event.op_type === "file.create") {
    addIdDetail(details, "Parent", metadata.parent_node_id);
    addByteDetail(details, "Size", metadata.byte_len_after);
    addNumberDetail(details, "Lines", metadata.line_count_after);
  } else if (event.op_type.startsWith("text.")) {
    addChangeDetail(details, "Size", metadata.byte_len_before, metadata.byte_len_after, formatBytes);
    addChangeDetail(details, "Lines", metadata.line_count_before, metadata.line_count_after, formatNumber);
  } else if (event.op_type === "item.move") {
    addIdDetail(details, "From parent", metadata.parent_node_id_before);
    addIdDetail(details, "To parent", metadata.parent_node_id_after);
    if (metadata.name_changed === true) details.push({ label: "Also renamed", value: "Yes" });
  } else if (event.op_type === "item.copy") {
    addIdDetail(details, "Source", metadata.copied_from_node_id);
    addIdDetail(details, "To parent", metadata.parent_node_id_after);
    addNumberDetail(details, "Copied items", metadata.copied_nodes);
    addNumberDetail(details, "Copied texts", metadata.copied_texts);
    addNumberDetail(details, "Copied files", metadata.copied_files);
    addBooleanDetail(details, "Recursive", metadata.recursive);
  } else if (event.op_type === "item.delete") {
    addNumberDetail(details, "Deleted items", metadata.deleted_nodes);
    addBooleanDetail(details, "Recursive", metadata.recursive);
  } else if (event.op_type === "item.update") {
    const changes = [
      metadata.name_changed === true ? "Name" : null,
      metadata.sort_order_changed === true ? "Order" : null
    ].filter((value): value is string => value !== null);
    if (changes.length > 0) details.push({ label: "Changed", value: changes.join(", ") });
  }

  if (event.node_id) details.push({ label: "Node", value: shortId(event.node_id) });
  return details;
}

function addIdDetail(details: FileChangeDetail[], label: string, value: unknown) {
  if (typeof value === "string") details.push({ label, value: shortId(value) });
}

function addByteDetail(details: FileChangeDetail[], label: string, value: unknown) {
  if (isFiniteNumber(value)) details.push({ label, value: formatBytes(value) });
}

function addNumberDetail(details: FileChangeDetail[], label: string, value: unknown) {
  if (isFiniteNumber(value)) details.push({ label, value: formatNumber(value) });
}

function addBooleanDetail(details: FileChangeDetail[], label: string, value: unknown) {
  if (typeof value === "boolean") details.push({ label, value: value ? "Yes" : "No" });
}

function addChangeDetail(
  details: FileChangeDetail[],
  label: string,
  before: unknown,
  after: unknown,
  format: (value: number) => string
) {
  if (isFiniteNumber(before) && isFiniteNumber(after)) {
    details.push({ label, value: `${format(before)} → ${format(after)}` });
  }
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function formatNumber(value: number): string {
  return numberFormatter.format(value);
}

export function formatActor(
  actor: AccountRef | null | undefined,
  actorAccountId: string | null
): string {
  if (actor?.display_name) return `${actor.display_name} (${actor.kind === "agent" ? "Agent" : "User"})`;
  const kind = actor?.kind === "agent" ? "Agent" : "Account";
  return `${kind} ${shortId(actor?.id ?? actorAccountId)}`;
}
