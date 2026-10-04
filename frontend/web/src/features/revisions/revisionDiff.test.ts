import { describe, expect, it } from "vitest";

import { compareRevisions } from "./revisionDiff";

function reconstruct(before: string, after: string) {
  const result = compareRevisions(before, after);
  expect(result.status).toBe("ready");
  if (result.status !== "ready") throw new Error("unexpected limit");
  expect(result.rows.flatMap((row) => row.before ? [row.before.text] : []).join("\n")).toBe(before);
  expect(result.rows.flatMap((row) => row.after ? [row.after.text] : []).join("\n")).toBe(after);
  return result.rows;
}

describe("revision line comparison", () => {
  it("aligns a replacement and insertion without losing unchanged context", () => {
    const rows = reconstruct("# Note\nMTU: 1500\nEnd", "# Note\nMTU: 1450\nReady\nEnd");
    expect(rows[0].kind).toBe("same");
    expect(rows[1]).toMatchObject({ kind: "change", before: { text: "MTU: 1500" }, after: { text: "MTU: 1450" } });
    expect(rows[2]).toMatchObject({ kind: "change", before: null, after: { number: 3, text: "Ready" } });
    expect(rows[3].kind).toBe("same");
  });
  it.each([
    ["", ""], ["", "new"], ["old", ""], ["a\na\nb", "a\nb\nb"],
    ["line\n", "line"], ["a\r\nb", "a\nb"], ["한글\n🙂", "한글\n새 내용\n🙂"]
  ])("preserves exact source including newline changes: %j -> %j", (before, after) => {
    reconstruct(before, after);
  });
  it("bounds input size and rendered line count before allocating the matrix", () => {
    expect(compareRevisions("a".repeat(256_001), "b")).toEqual({ status: "limited" });
    expect(compareRevisions("x\n".repeat(1_500), "b")).toEqual({ status: "limited" });
  });
});
