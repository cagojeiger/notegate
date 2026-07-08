const LANGUAGE_ALIASES: Record<string, string> = {
  md: "markdown",
  text: "text",
  txt: "text",
  yml: "yaml"
};

const LANGUAGE_LABELS: Record<string, string> = {
  bash: "Bash",
  css: "CSS",
  html: "HTML",
  js: "JavaScript",
  json: "JSON",
  jsx: "JSX",
  markdown: "Markdown",
  py: "Python",
  rs: "Rust",
  sh: "Shell",
  sql: "SQL",
  text: "Text",
  ts: "TypeScript",
  tsx: "TSX",
  yaml: "YAML",
  zsh: "Zsh"
};

export function normalizeCodeLanguage(language: string): string {
  const normalized = language.toLowerCase();
  return LANGUAGE_ALIASES[normalized] ?? normalized;
}

export function formatCodeBlockLabel(language: string): string {
  return LANGUAGE_LABELS[normalizeCodeLanguage(language)] ?? language;
}
