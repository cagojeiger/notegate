export type TextFormat = "markdown" | "json" | "jsonl" | "yaml" | "toml" | "plain";

const FORMAT_BY_EXTENSION: Record<string, TextFormat> = {
  md: "markdown",
  markdown: "markdown",
  json: "json",
  jsonl: "jsonl",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml"
};

export function inferTextFormat(name: string): TextFormat {
  const dot = name.lastIndexOf(".");
  if (dot < 0 || dot === name.length - 1) return "plain";
  const extension = name.slice(dot + 1).toLowerCase();
  return FORMAT_BY_EXTENSION[extension] ?? "plain";
}

export function shikiLangForFormat(format: TextFormat): string {
  if (format === "markdown") return "markdown";
  if (format === "yaml") return "yaml";
  if (format === "jsonl") return "jsonl";
  if (format === "toml") return "toml";
  if (format === "json") return "json";
  return "text";
}

export function isStructuredFormat(format: TextFormat): format is "json" | "jsonl" | "yaml" | "toml" {
  return format === "json" || format === "jsonl" || format === "yaml" || format === "toml";
}
