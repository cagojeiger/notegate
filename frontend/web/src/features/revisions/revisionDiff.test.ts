import { describe, expect, it } from "vitest";

import { compareRevisions, describeRevisionDiff } from "./revisionDiff";

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

describe("revision change sections", () => {
  it("hides a long unchanged prefix and keeps three context lines without losing expandable rows", () => {
    const before = Array.from({ length: 500 }, (_, index) => `Line ${index + 1}`);
    const after = before.map((line, index) => index === 120 || index === 480 ? `${line} changed` : line);
    const rows = reconstruct(before.join("\n"), after.join("\n"));
    const view = describeRevisionDiff(rows);
    expect(view).toMatchObject({ added: 2, removed: 2, changes: [
      { start: 117, end: 124 }, { start: 477, end: 484 }
    ] });
    expect(view.sections.map(({ kind, start, end }) => [kind, end - start])).toEqual([
      ["unchanged", 117], ["changes", 7], ["unchanged", 353], ["changes", 7], ["unchanged", 16]
    ]);
    expect(view.sections.flatMap(({ start, end }) => rows.slice(start, end))).toEqual(rows);
  });

  it.each([6, 7])("merges nearby changes %i lines apart without duplicate context", (distance) => {
    const before = Array.from({ length: 24 }, (_, index) => `Line ${index}`);
    const after = before.map((line, index) => index === 5 || index === 5 + distance ? `${line} changed` : line);
    const rows = reconstruct(before.join("\n"), after.join("\n"));
    const view = describeRevisionDiff(rows);
    expect(view.changes).toEqual([{ kind: "changes", start: 2, end: 9 + distance }]);
    expect(view.sections.flatMap(({ start, end }) => rows.slice(start, end))).toEqual(rows);
  });

  it("counts each side separately and bounds context at document edges", () => {
    const rows = reconstruct("old\nkeep\nend", "new\nextra\nkeep");
    expect(describeRevisionDiff(rows)).toMatchObject({ added: 2, removed: 2, changes: [{ start: 0, end: rows.length }] });
  });

  it.each([
    ["", "new", 1, 0], ["old", "", 0, 1],
    ["line", "line\n", 0, 0], ["line\n", "line", 0, 0],
    ["", "\n", 1, 0], ["\n", "", 0, 1],
    ["line\n", "line\n\n", 1, 0], ["line\n\n", "line\n", 0, 1]
  ])("counts logical lines while retaining newline-only changes: %j -> %j", (before, after, added, removed) => {
    const view = describeRevisionDiff(reconstruct(before, after));
    expect(view).toMatchObject({ added, removed });
    expect(view.changes).not.toHaveLength(0);
  });

  it("does not invent a change for identical content, including an empty document", () => {
    for (const text of ["", "same\ncontent\n"]) {
      expect(describeRevisionDiff(reconstruct(text, text))).toMatchObject({ added: 0, removed: 0, changes: [] });
    }
  });
});
