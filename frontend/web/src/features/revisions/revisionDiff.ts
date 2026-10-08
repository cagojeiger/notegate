// Bounded line LCS. This runs in a disposable Worker, never in the editor render path.
export type DiffLine = { number: number; text: string };
export type DiffRow = { kind: "same" | "change"; before: DiffLine | null; after: DiffLine | null };
export type RevisionDiff = { status: "ready"; rows: DiffRow[] } | { status: "limited" };
export type DiffSection = { kind: "changes" | "unchanged"; start: number; end: number };
const MAX_CHARS = 256_000;
const MAX_LINES = 1_500;
const MAX_CELLS = 1_000_000;

// Half-open row ranges keep three context lines around each change. Nearby
// changes share one section; expanding a gap never needs another body read.
export function describeRevisionDiff(rows: DiffRow[]) {
  const changes: DiffSection[] = [];
  const lastBefore = rows.reduce<DiffLine | null>((last, row) => row.before ?? last, null);
  const lastAfter = rows.reduce<DiffLine | null>((last, row) => row.after ?? last, null);
  let added = 0;
  let removed = 0;
  rows.forEach((row, index) => {
    if (row.kind === "same") return;
    // A final split("\n") placeholder preserves newline changes, but is not a
    // logical line (including the sole placeholder of an empty document).
    if (row.before && (row.before.text !== "" || row.before !== lastBefore)) removed++;
    if (row.after && (row.after.text !== "" || row.after !== lastAfter)) added++;
    const start = Math.max(0, index - 3);
    const end = Math.min(rows.length, index + 4);
    const previous = changes[changes.length - 1];
    if (previous && start <= previous.end) previous.end = end;
    else changes.push({ kind: "changes", start, end });
  });
  const sections: DiffSection[] = [];
  let end = 0;
  for (const change of changes) {
    if (end < change.start) sections.push({ kind: "unchanged", start: end, end: change.start });
    sections.push(change);
    end = change.end;
  }
  if (end < rows.length) sections.push({ kind: "unchanged", start: end, end: rows.length });
  return { sections, changes, added, removed };
}

export function compareRevisions(before: string, after: string): RevisionDiff {
  if (before.length + after.length > MAX_CHARS) return { status: "limited" };
  // Preserve trailing empty lines and CR characters: newline-only changes must be visible.
  const left = before.split("\n");
  const right = after.split("\n");
  const leftLineCount = left.length - (left[left.length - 1] === "" ? 1 : 0);
  const rightLineCount = right.length - (right[right.length - 1] === "" ? 1 : 0);
  // Do not align a trailing placeholder with an actual blank line.
  const same = (i: number, j: number) => left[i] === right[j] && (i < leftLineCount) === (j < rightLineCount);
  if (left.length + right.length > MAX_LINES) return { status: "limited" };
  const width = right.length + 1;
  if ((left.length + 1) * width > MAX_CELLS) return { status: "limited" };
  const lengths = new Uint16Array((left.length + 1) * width);
  for (let i = left.length - 1; i >= 0; i--) {
    for (let j = right.length - 1; j >= 0; j--) {
      lengths[i * width + j] = same(i, j)
        ? lengths[(i + 1) * width + j + 1] + 1
        : Math.max(lengths[(i + 1) * width + j], lengths[i * width + j + 1]);
    }
  }
  const rows: DiffRow[] = [];
  let i = 0;
  let j = 0;
  let removed: DiffLine[] = [];
  let added: DiffLine[] = [];
  function flush() {
    for (let k = 0; k < Math.max(removed.length, added.length); k++) {
      rows.push({ kind: "change", before: removed[k] ?? null, after: added[k] ?? null });
    }
    removed = [];
    added = [];
  }
  while (i < left.length || j < right.length) {
    if (i < left.length && j < right.length && same(i, j)) {
      flush();
      rows.push({ kind: "same", before: { number: i + 1, text: left[i++] }, after: { number: j + 1, text: right[j++] } });
    } else if (i < left.length && (j === right.length || lengths[(i + 1) * width + j] >= lengths[i * width + j + 1])) {
      removed.push({ number: i + 1, text: left[i++] });
    } else {
      added.push({ number: j + 1, text: right[j++] });
    }
  }
  flush();
  return { status: "ready", rows };
}
