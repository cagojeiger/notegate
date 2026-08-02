export function hasMarkdownFrontmatterCandidate(content: string): boolean {
  const firstLineEnd = content.indexOf("\n");
  const firstLine = content
    .slice(0, firstLineEnd === -1 ? content.length : firstLineEnd)
    .replace(/^\uFEFF/, "")
    .replace(/\r$/, "");

  return /^---[ \t]*$/.test(firstLine);
}

export function formatFrontmatterValue(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return value.map(formatFrontmatterValue).join(", ");
  if (typeof value === "object") return JSON.stringify(value) ?? String(value);
  return String(value);
}
