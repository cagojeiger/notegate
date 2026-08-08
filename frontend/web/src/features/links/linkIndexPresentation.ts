import type { LinkSyncStatus } from "../../api/types";

export function linkIndexStatusLabel(status: LinkSyncStatus) {
  switch (status) {
    case "up_to_date": return "Up to date";
    case "pending": return "Waiting";
    case "syncing": return "Syncing";
    case "retrying": return "Retrying";
  }
}

export function formatLinkIndexSyncTime(value: string | null) {
  if (!value) return "Not synced yet";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value));
}
