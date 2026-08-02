import { readFile } from "node:fs/promises";
import { gzipSync } from "node:zlib";

const manifestUrl = new URL("../dist/.vite/manifest.json", import.meta.url);
const manifest = JSON.parse(await readFile(manifestUrl, "utf8"));
const entryKey = Object.keys(manifest).find((key) =>
  key === "src/features/editor/StructuredPreview.tsx" || manifest[key].name === "StructuredPreview"
);
const entry = manifest[entryKey];

if (!entry?.file) {
  throw new Error("Missing Vite manifest entry: StructuredPreview");
}

const initialKey = Object.keys(manifest).find((key) => manifest[key].isEntry);
if (!initialKey) {
  throw new Error("Missing initial Vite manifest entry");
}

const initialFiles = collectStaticJavaScript(initialKey);
const previewFiles = [...collectStaticJavaScript(entryKey)].filter((file) => !initialFiles.has(file));
const [initialAssets, previewAssets] = await Promise.all([
  measureAssets(initialFiles),
  measureAssets(previewFiles)
]);
const initialGzipBytes = totalGzipBytes(initialAssets);
const previewGzipBytes = totalGzipBytes(previewAssets);
const maxInitialGzipBytes = 120_000;
const maxPreviewGzipBytes = 15_000;

printMeasurement("Initial JavaScript", initialGzipBytes, maxInitialGzipBytes, initialAssets);
printMeasurement("JSON/JSONL preview incremental JavaScript", previewGzipBytes, maxPreviewGzipBytes, previewAssets);

if (initialGzipBytes > maxInitialGzipBytes || previewGzipBytes > maxPreviewGzipBytes) {
  process.exitCode = 1;
}

async function measureAssets(files) {
  return Promise.all([...files].map(async (file) => {
    const contents = await readFile(new URL(`../dist/${file}`, import.meta.url));
    return { file, gzipBytes: gzipSync(contents).byteLength };
  }));
}

function totalGzipBytes(assets) {
  return assets.reduce((total, asset) => total + asset.gzipBytes, 0);
}

function printMeasurement(label, gzipBytes, maxGzipBytes, assets) {
  console.log(`${label}: ${gzipBytes.toLocaleString("en-US")} B gzip`);
  for (const asset of assets.sort((left, right) => right.gzipBytes - left.gzipBytes)) {
    console.log(`  ${asset.gzipBytes.toLocaleString("en-US").padStart(10)} B  ${asset.file}`);
  }
  if (gzipBytes > maxGzipBytes) {
    console.error(`Expected at most ${maxGzipBytes.toLocaleString("en-US")} B gzip`);
  }
}

function collectStaticJavaScript(key, files = new Set(), visited = new Set()) {
  if (visited.has(key)) return files;
  visited.add(key);

  const chunk = manifest[key];
  if (!chunk) throw new Error(`Missing imported Vite manifest entry: ${key}`);
  if (/\.(?:m?js)$/.test(chunk.file)) files.add(chunk.file);

  for (const importedKey of chunk.imports ?? []) {
    collectStaticJavaScript(importedKey, files, visited);
  }
  return files;
}
