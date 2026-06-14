import { describe, expect, it } from "vitest";

import { inferTextFormat, shikiLangForFormat } from "./textFormat";

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

  it("maps formats to highlighter languages", () => {
    expect(shikiLangForFormat("markdown")).toBe("markdown");
    expect(shikiLangForFormat("jsonl")).toBe("jsonl");
    expect(shikiLangForFormat("plain")).toBe("text");
  });
});
