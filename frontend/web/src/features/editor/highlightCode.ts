import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import json from "shiki/langs/json.mjs";
import jsonl from "shiki/langs/jsonl.mjs";
import markdown from "shiki/langs/markdown.mjs";
import toml from "shiki/langs/toml.mjs";
import yaml from "shiki/langs/yaml.mjs";
import minLight from "shiki/themes/min-light.mjs";
import nightOwl from "shiki/themes/night-owl.mjs";

const SUPPORTED_LANGUAGES = new Set(["json", "jsonl", "markdown", "toml", "yaml"]);

let highlighterPromise: Promise<HighlighterCore> | null = null;

export async function highlightCode(code: string, language: string): Promise<string> {
  if (!SUPPORTED_LANGUAGES.has(language)) {
    throw new Error(`unsupported highlight language: ${language}`);
  }
  const lang = language;
  const highlighter = await getHighlighter();
  return highlighter.codeToHtml(code, {
    lang,
    themes: { light: "min-light", dark: "night-owl" },
    defaultColor: false
  });
}

function getHighlighter(): Promise<HighlighterCore> {
  highlighterPromise ??= createHighlighterCore({
    themes: [minLight, nightOwl],
    langs: [json, jsonl, markdown, toml, yaml],
    engine: createJavaScriptRegexEngine()
  });
  return highlighterPromise;
}
