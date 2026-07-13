import type { AccountRef, AuditEvent, FileChangeEvent } from "../../api/types";

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

export function formatActor(
  actor: AccountRef | null | undefined,
  actorAccountId: string | null
): string {
  if (actor?.display_name) return `${actor.display_name} (${actor.kind === "agent" ? "Agent" : "User"})`;
  const kind = actor?.kind === "agent" ? "Agent" : "Account";
  return `${kind} ${shortId(actor?.id ?? actorAccountId)}`;
}
