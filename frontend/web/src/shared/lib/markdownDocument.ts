import { parse as parseYaml } from "yaml";

import { hasMarkdownFrontmatterCandidate } from "./markdownFrontmatter";

export type MarkdownDocument = {
  frontmatter: Record<string, unknown> | null;
  body: string;
};

export function parseMarkdownDocument(content: string): MarkdownDocument {
  if (!hasMarkdownFrontmatterCandidate(content)) {
    return { frontmatter: null, body: content };
  }

  const firstLineEnd = content.indexOf("\n");
  const frontmatterStart = firstLineEnd === -1 ? content.length : firstLineEnd + 1;
  const closingFence = findClosingFence(content, frontmatterStart);
  if (!closingFence) return { frontmatter: null, body: content };

  const source = content.slice(frontmatterStart, closingFence.start);
  try {
    const parsed = parseYaml(source);
    if (parsed == null) return { frontmatter: {}, body: content.slice(closingFence.end) };
    if (!isPlainRecord(parsed)) return { frontmatter: null, body: content };
    return { frontmatter: parsed, body: content.slice(closingFence.end) };
  } catch {
    return { frontmatter: null, body: content };
  }
}

function findClosingFence(content: string, startIndex: number): { start: number; end: number } | null {
  let index = startIndex;

  while (index < content.length) {
    const lineEnd = content.indexOf("\n", index);
    const lineStop = lineEnd === -1 ? content.length : lineEnd;
    const line = content.slice(index, lineStop).replace(/\r$/, "");

    if (/^(?:---|\.\.\.)[ \t]*$/.test(line)) {
      return { start: index, end: lineEnd === -1 ? content.length : lineEnd + 1 };
    }

    if (lineEnd === -1) break;
    index = lineEnd + 1;
  }

  return null;
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}
