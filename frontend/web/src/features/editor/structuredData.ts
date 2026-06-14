import { parse as parseToml, TomlDate } from "smol-toml";
import { parse as parseYaml } from "yaml";

export type StructuredFormat = "json" | "jsonl" | "yaml" | "toml";

export type StructuredParseResult =
  | { ok: true; value: Record<string, unknown> | unknown[]; label: string }
  | { ok: false; message: string };

export function parseStructuredText(format: StructuredFormat, content: string): StructuredParseResult {
  try {
    const value = normalizeForTree(parseByFormat(format, content));
    return { ok: true, value: wrapScalar(value), label: labelForFormat(format) };
  } catch (error) {
    return { ok: false, message: error instanceof Error ? error.message : String(error) };
  }
}

function parseByFormat(format: StructuredFormat, content: string): unknown {
  if (format === "json") return JSON.parse(content);
  if (format === "jsonl") return parseJsonl(content);
  if (format === "yaml") return parseYaml(content);
  return parseToml(content);
}

function parseJsonl(content: string): unknown[] {
  return content.split(/\r?\n/).filter((line, index, lines) => !(index === lines.length - 1 && line === "")).map((line, index) => ({
    line: index + 1,
    value: JSON.parse(line)
  }));
}

function labelForFormat(format: StructuredFormat): string {
  if (format === "jsonl") return "JSONL records";
  return format.toUpperCase();
}

function wrapScalar(value: unknown): Record<string, unknown> | unknown[] {
  if (Array.isArray(value)) return value;
  if (value && typeof value === "object") return value as Record<string, unknown>;
  return { value };
}

function normalizeForTree(value: unknown): unknown {
  if (typeof value === "bigint") return value.toString();
  if (value instanceof Date || value instanceof TomlDate) return value.toString();
  if (Array.isArray(value)) return value.map(normalizeForTree);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, normalizeForTree(child)]));
  }
  return value;
}
