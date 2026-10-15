import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { gzipSync } from "node:zlib";

test("counts workspace once and measures only additional preview assets", async (t) => {
  const { result, assets } = await runFixture(t);
  assert.equal(result.status, 0, result.stderr);
  const gzipBytes = (...names) => names.reduce((total, name) => total + gzipSync(assets[name]).byteLength, 0);
  assert.ok(result.stdout.includes(`Initial JavaScript: ${gzipBytes("index", "react").toLocaleString("en-US")} B gzip`));
  assert.ok(result.stdout.includes(`Authenticated workspace total JavaScript (including initial): ${gzipBytes("index", "react", "workspace", "helper").toLocaleString("en-US")} B gzip`));
  assert.ok(result.stdout.includes(`JSON/JSONL preview incremental JavaScript: ${gzipBytes("structured", "preview-shared").toLocaleString("en-US")} B gzip`));
  assert.ok(result.stdout.includes(`Frontmatter-free Markdown preview incremental JavaScript: ${gzipBytes("markdown", "preview-shared").toLocaleString("en-US")} B gzip`));
  assert.ok(result.stdout.includes(`DOCX preview incremental JavaScript: ${gzipBytes("docx").toLocaleString("en-US")} B gzip`));
});

for (const [asset, budget] of [["workspace", "180,000"], ["structured", "15,000"], ["markdown", "60,000"], ["docx", "60,000"]]) {
  test(`still rejects excessive ${asset} JavaScript`, async (t) => {
    const { result } = await runFixture(t, { [asset]: 300_000 });
    assert.equal(result.status, 1, result.stderr);
    assert.equal(result.stderr.trim(), `Expected at most ${budget} B gzip`);
  });
}

test("requires the authenticated workspace entry instead of silently undercounting", async (t) => {
  const { result } = await runFixture(t, {}, false);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Missing Vite manifest entry: AuthenticatedWorkspace/);
});

async function runFixture(t, sizes = {}, includeWorkspace = true) {
  const root = await mkdtemp(join(tmpdir(), "notegate-preview-bundle-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await Promise.all([
    mkdir(join(root, "scripts")),
    mkdir(join(root, "dist", ".vite"), { recursive: true }),
    mkdir(join(root, "dist", "assets"), { recursive: true })
  ]);
  const script = join(root, "scripts", "check-preview-bundle.mjs");
  await copyFile(new URL("./check-preview-bundle.mjs", import.meta.url), script);
  const assets = Object.fromEntries(Object.entries({
    index: 5_000, react: 1_000, workspace: 55_000, helper: 1_500,
    structured: 1_000, markdown: 2_000, docx: 1_000, "preview-shared": 400,
    ...sizes
  }).map(([name, size]) => [name, fixtureAsset(name, size)]));
  const chunk = (name, imports = []) => ({ file: `assets/${name}.js`, imports });
  const manifest = {
    "index.html": { ...chunk("index", ["react"]), isEntry: true, dynamicImports: ["workspace"] },
    react: chunk("react"),
    workspace: {
      ...chunk("workspace", ["react", "helper"]), name: "AuthenticatedWorkspace",
      dynamicImports: ["structured", "markdown", "docx", "unused"]
    },
    // Shared imports and a cycle must not count an asset more than once.
    helper: chunk("helper", ["workspace"]),
    structured: { ...chunk("structured", ["workspace", "preview-shared"]), name: "StructuredPreview" },
    markdown: { ...chunk("markdown", ["workspace", "preview-shared"]), name: "MarkdownPreview" },
    docx: { ...chunk("docx", ["react"]), name: "DocxPreview" },
    "preview-shared": chunk("preview-shared"),
    // No file is written: unopened dynamic imports must not be measured.
    unused: chunk("unused")
  };
  if (!includeWorkspace) delete manifest.workspace;
  await Promise.all([
    writeFile(join(root, "dist", ".vite", "manifest.json"), JSON.stringify(manifest)),
    ...Object.entries(assets).map(([name, content]) => writeFile(join(root, "dist", "assets", `${name}.js`), content))
  ]);
  return { result: spawnSync(process.execPath, [script], { encoding: "utf8", timeout: 10_000 }), assets };
}

function fixtureAsset(name, size) {
  // Deterministic, poorly compressible data keeps budget failures reproducible.
  const blocks = [];
  for (let index = 0; index < Math.ceil(size / 32); index += 1) {
    blocks.push(createHash("sha256").update(`${name}:${index}`).digest());
  }
  return Buffer.concat(blocks).subarray(0, size);
}
