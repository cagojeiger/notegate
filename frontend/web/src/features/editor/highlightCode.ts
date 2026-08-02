import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import githubDarkDimmed from "shiki/themes/github-dark-dimmed.mjs";
import githubLight from "shiki/themes/github-light.mjs";

const LANGUAGE_LOADERS = {
  go: () => import("shiki/langs/go.mjs"),
  javascript: () => import("shiki/langs/javascript.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  jsonl: () => import("shiki/langs/jsonl.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  rust: () => import("shiki/langs/rust.mjs"),
  shellscript: () => import("shiki/langs/shellscript.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  typescript: () => import("shiki/langs/typescript.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs")
} as const;

type SupportedLanguage = keyof typeof LANGUAGE_LOADERS;

let highlighterPromise: Promise<HighlighterCore> | null = null;
const languageLoadPromises = new Map<SupportedLanguage, Promise<void>>();

export async function highlightCode(code: string, language: string): Promise<string> {
  if (!isSupportedLanguage(language)) {
    throw new Error(`unsupported highlight language: ${language}`);
  }
  const highlighter = await getHighlighter();
  await loadLanguage(highlighter, language);
  return highlighter.codeToHtml(code, {
    lang: language,
    themes: { light: "github-light", dark: "github-dark-dimmed" },
    defaultColor: false,
    transformers: [{
      pre(element) {
        element.properties.tabIndex = 0;
      }
    }]
  });
}

function getHighlighter(): Promise<HighlighterCore> {
  highlighterPromise ??= createHighlighterCore({
    themes: [githubLight, githubDarkDimmed],
    langs: [],
    engine: createJavaScriptRegexEngine()
  });
  return highlighterPromise;
}

function loadLanguage(highlighter: HighlighterCore, language: SupportedLanguage): Promise<void> {
  let loadPromise = languageLoadPromises.get(language);
  if (!loadPromise) {
    loadPromise = LANGUAGE_LOADERS[language]().then((module) => highlighter.loadLanguage(module.default));
    languageLoadPromises.set(language, loadPromise);
  }
  return loadPromise;
}

function isSupportedLanguage(language: string): language is SupportedLanguage {
  return Object.prototype.hasOwnProperty.call(LANGUAGE_LOADERS, language);
}
