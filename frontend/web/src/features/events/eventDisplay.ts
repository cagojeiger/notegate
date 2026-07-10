const METADATA_PREVIEW_LIMIT = 180;

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

export function formatMetadata(metadata: Record<string, unknown> | null | undefined): string {
  if (!metadata || Object.keys(metadata).length === 0) return "—";
  const text = Object.entries(metadata)
    .map(([key, value]) => `${key}=${formatMetadataValue(value)}`)
    .join(" · ");
  return text.length > METADATA_PREVIEW_LIMIT ? `${text.slice(0, METADATA_PREVIEW_LIMIT - 1)}…` : text;
}

function formatMetadataValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value === null) return "null";
  return JSON.stringify(value);
}
