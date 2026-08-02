export type CodeFormat = "go" | "javascript" | "jsx" | "python" | "rust" | "shellscript" | "sql" | "tsx" | "typescript";
export type TextFormat = "markdown" | "json" | "jsonl" | "yaml" | "toml" | CodeFormat | "plain";

const FORMAT_BY_EXTENSION: Record<string, TextFormat> = {
  md: "markdown",
  markdown: "markdown",
  json: "json",
  jsonl: "jsonl",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  go: "go",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  rs: "rust",
  sh: "shellscript",
  bash: "shellscript",
  zsh: "shellscript",
  ksh: "shellscript",
  sql: "sql",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "tsx",
  py: "python",
  pyi: "python",
  pyw: "python"
};

const CODE_FORMATS = new Set<TextFormat>(["go", "javascript", "jsx", "python", "rust", "shellscript", "sql", "tsx", "typescript"]);

export function inferTextFormat(name: string): TextFormat {
  const dot = name.lastIndexOf(".");
  if (dot < 0 || dot === name.length - 1) return "plain";
  const extension = name.slice(dot + 1).toLowerCase();
  return FORMAT_BY_EXTENSION[extension] ?? "plain";
}

export function shikiLangForFormat(format: TextFormat): string {
  return format === "plain" ? "text" : format;
}

export function isStructuredFormat(format: TextFormat): format is "json" | "jsonl" | "yaml" | "toml" {
  return format === "json" || format === "jsonl" || format === "yaml" || format === "toml";
}

export function isCodeFormat(format: TextFormat): format is CodeFormat {
  return CODE_FORMATS.has(format);
}
