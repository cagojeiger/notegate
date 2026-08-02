import { describe, expect, it } from "vitest";

import { inferTextFormat, isCodeFormat, isStructuredFormat, shikiLangForFormat } from "./textFormat";

describe("textFormat", () => {
  it("infers known document formats from file names", () => {
    expect(inferTextFormat("README.md")).toBe("markdown");
    expect(inferTextFormat("data.JSON")).toBe("json");
    expect(inferTextFormat("events.jsonl")).toBe("jsonl");
    expect(inferTextFormat("config.yaml")).toBe("yaml");
    expect(inferTextFormat("config.yml")).toBe("yaml");
    expect(inferTextFormat("Cargo.toml")).toBe("toml");
    expect(inferTextFormat("notes")).toBe("plain");
  });

  it.each([
    ["main.GO", "go"],
    ["app.js", "javascript"],
    ["worker.MJS", "javascript"],
    ["config.cjs", "javascript"],
    ["view.jsx", "jsx"],
    ["lib.RS", "rust"],
    ["deploy.sh", "shellscript"],
    ["profile.bash", "shellscript"],
    ["hooks.zsh", "shellscript"],
    ["job.ksh", "shellscript"],
    ["query.SQL", "sql"],
    ["app.ts", "typescript"],
    ["worker.MTS", "typescript"],
    ["config.cts", "typescript"],
    ["view.tsx", "tsx"],
    ["script.Py", "python"],
    ["types.PYI", "python"],
    ["launcher.pyw", "python"]
  ] as const)("infers %s as %s case-insensitively", (name, format) => {
    expect(inferTextFormat(name)).toBe(format);
  });

  it("maps formats to highlighter languages", () => {
    expect(shikiLangForFormat("markdown")).toBe("markdown");
    expect(shikiLangForFormat("jsonl")).toBe("jsonl");
    expect(shikiLangForFormat("go")).toBe("go");
    expect(shikiLangForFormat("javascript")).toBe("javascript");
    expect(shikiLangForFormat("jsx")).toBe("jsx");
    expect(shikiLangForFormat("rust")).toBe("rust");
    expect(shikiLangForFormat("shellscript")).toBe("shellscript");
    expect(shikiLangForFormat("sql")).toBe("sql");
    expect(shikiLangForFormat("typescript")).toBe("typescript");
    expect(shikiLangForFormat("tsx")).toBe("tsx");
    expect(shikiLangForFormat("python")).toBe("python");
    expect(shikiLangForFormat("plain")).toBe("text");
  });

  it("keeps source formats separate from structured previews", () => {
    for (const format of ["go", "javascript", "jsx", "python", "rust", "shellscript", "sql", "tsx", "typescript"] as const) {
      expect(isCodeFormat(format)).toBe(true);
    }
    expect(isStructuredFormat("sql")).toBe(false);
    expect(isStructuredFormat("python")).toBe(false);
    expect(isStructuredFormat("json")).toBe(true);
  });
});
