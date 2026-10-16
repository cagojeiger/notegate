export function hasMarkdownFrontmatterCandidate(content: string): boolean {
  const firstLineEnd = content.indexOf("\n");
  const firstLine = content
    .slice(0, firstLineEnd === -1 ? content.length : firstLineEnd)
    .replace(/^\uFEFF/, "")
    .replace(/\r$/, "");

  return /^---[ \t]*$/.test(firstLine);
}

export function formatFrontmatterValue(value: unknown): string {
  const ancestors = new Set<object>();
  let remaining = 1_000;
  let fallback = "Value too complex to display";

  function canFormat(value: unknown, depth: number): boolean {
    if (--remaining < 0 || depth > 32) return false;
    if (value === null || typeof value !== "object") return true;
    if (ancestors.has(value)) {
      fallback = "Circular reference";
      return false;
    }
    ancestors.add(value);
    const valid = Object.values(value).every((child) => canFormat(child, depth + 1));
    ancestors.delete(value);
    return valid;
  }

  return canFormat(value, 0) ? formatValue(value) : `[${fallback}]`;
}

function formatValue(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return value.map(formatValue).join(", ");
  if (typeof value === "object") return JSON.stringify(value) ?? String(value);
  return String(value);
}
