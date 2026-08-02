const LANGUAGE_ALIASES: Record<string, string> = {
  bash: "shellscript",
  cjs: "javascript",
  cts: "typescript",
  js: "javascript",
  ksh: "shellscript",
  md: "markdown",
  mjs: "javascript",
  mts: "typescript",
  py: "python",
  rs: "rust",
  sh: "shellscript",
  shell: "shellscript",
  text: "text",
  ts: "typescript",
  txt: "text",
  yml: "yaml",
  zsh: "shellscript"
};

const LANGUAGE_LABELS: Record<string, string> = {
  css: "CSS",
  go: "Go",
  html: "HTML",
  javascript: "JavaScript",
  json: "JSON",
  jsx: "JSX",
  markdown: "Markdown",
  python: "Python",
  rust: "Rust",
  shellscript: "Shell",
  sql: "SQL",
  text: "Text",
  typescript: "TypeScript",
  tsx: "TSX",
  yaml: "YAML"
};

export function normalizeCodeLanguage(language: string): string {
  const normalized = language.toLowerCase();
  return LANGUAGE_ALIASES[normalized] ?? normalized;
}

export function formatCodeBlockLabel(language: string): string {
  return LANGUAGE_LABELS[normalizeCodeLanguage(language)] ?? language;
}
