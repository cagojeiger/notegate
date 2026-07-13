import type { AccountRef, AuditEvent, FileChangeEvent } from "../../api/types";

const FILE_CHANGE_ACTIONS: Record<string, string> = {
  "folder.create": "Created a folder",
  "text.create": "Created a text item",
  "file.create": "Added a file",
  "text.write": "Updated text",
  "text.append": "Appended text",
  "text.patch": "Patched text",
  "text.line_edit": "Edited text lines",
  "item.update": "Updated an item",
  "item.move": "Moved an item",
  "item.copy": "Copied an item",
  "item.delete": "Deleted an item",
  "metadata.replace": "Replaced metadata",
  "metadata.patch": "Updated metadata"
};

const AUDIT_ACTIONS: Record<string, string> = {
  "account.create": "Created the account",
  "account.delete": "Deleted the account",
  "session.login": "Signed in",
  "session.logout": "Signed out",
  "session.revoke": "Session access ended",
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

export function shortId(value: string | null | undefined): string {
  if (!value) return "—";
  return value.length <= 12 ? value : `${value.slice(0, 8)}…${value.slice(-4)}`;
}

export function formatFileChangeAction(event: FileChangeEvent): string {
  return FILE_CHANGE_ACTIONS[event.op_type] ?? event.op_type;
}

export function formatAuditAction(event: AuditEvent): string {
  return AUDIT_ACTIONS[event.op_type] ?? event.op_type;
}

export function formatAuditTarget(event: AuditEvent): string {
  const label = event.resource_type.replace(/_/g, " ");
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
  return `${kind} ${name}`;
}

export function formatActor(
  actor: AccountRef | null | undefined,
  actorAccountId: string | null
): string {
  if (actor?.display_name) return `${actor.display_name} (${actor.kind === "agent" ? "Agent" : "User"})`;
  const kind = actor?.kind === "agent" ? "Agent" : "Account";
  return `${kind} ${shortId(actor?.id ?? actorAccountId)}`;
}
