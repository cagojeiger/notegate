import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import ts from "typescript";

const WEB_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE_ROOT = path.join(WEB_ROOT, "src");
const SOURCE_EXTENSIONS = [".ts", ".tsx"];

const ALLOWED_TARGETS = new Map([
  ["shared", new Set(["shared"])],
  ["entities", new Set(["entities"])],
  ["design", new Set(["design"])],
  ["api", new Set(["api", "entities", "shared"])],
  ["auth", new Set(["auth", "api", "design", "entities", "shared"])],
  ["stores", new Set(["stores", "design", "entities", "shared"])],
  ["features", new Set(["features", "api", "auth", "design", "entities", "shared", "stores"])],
  ["layout", new Set(["layout", "auth", "design", "entities", "features", "shared"])]
]);
const FORBIDDEN_SUBTREES = new Map([
  ["api", ["shared/ui"]],
  ["stores", ["shared/ui"]]
]);

const sourceFiles = collectSourceFiles(SOURCE_ROOT);
const sourceFileSet = new Set(sourceFiles);
const graph = new Map(sourceFiles.map((file) => [file, new Set()]));
const violations = [];

for (const sourceFile of sourceFiles) {
  const sourceLayer = layerOf(sourceFile);
  const parsed = ts.createSourceFile(
    sourceFile,
    fs.readFileSync(sourceFile, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    sourceFile.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );

  for (const moduleReference of moduleReferences(parsed)) {
    if (!moduleReference.specifier.startsWith(".")) continue;
    const targetFile = resolveSourceFile(sourceFile, moduleReference.specifier, sourceFileSet);
    if (!targetFile) continue;

    graph.get(sourceFile).add(targetFile);
    const allowedTargets = ALLOWED_TARGETS.get(sourceLayer);
    const targetLayer = layerOf(targetFile);
    const targetPath = path.relative(SOURCE_ROOT, targetFile).split(path.sep).join("/");
    const forbiddenSubtree = FORBIDDEN_SUBTREES.get(sourceLayer)?.find((prefix) => targetPath.startsWith(`${prefix}/`));
    if (
      (allowedTargets && sourceLayer !== targetLayer && !allowedTargets.has(targetLayer))
      || forbiddenSubtree
    ) {
      const { line } = parsed.getLineAndCharacterOfPosition(moduleReference.position);
      violations.push(
        `${relative(sourceFile)}:${line + 1} ${sourceLayer} cannot import ${forbiddenSubtree ?? targetLayer} (${moduleReference.specifier})`
      );
    }
  }
}

for (const cycle of findCycles(graph)) {
  violations.push(`cycle: ${cycle.map(relative).join(" -> ")}`);
}

if (violations.length > 0) {
  console.error("Frontend dependency boundary check failed:");
  for (const violation of violations) console.error(`- ${violation}`);
  process.exit(1);
}

const edgeCount = [...graph.values()].reduce((total, targets) => total + targets.size, 0);
console.log(`Frontend dependency boundaries OK (${sourceFiles.length} files, ${edgeCount} internal imports).`);

function collectSourceFiles(directory) {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const absolutePath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...collectSourceFiles(absolutePath));
    else if (SOURCE_EXTENSIONS.some((extension) => entry.name.endsWith(extension))) files.push(absolutePath);
  }
  return files.sort();
}

function layerOf(file) {
  return path.relative(SOURCE_ROOT, file).split(path.sep)[0];
}

function relative(file) {
  return path.relative(WEB_ROOT, file);
}

function moduleReferences(sourceFile) {
  const references = [];
  visit(sourceFile);
  return references;

  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node))
      && node.moduleSpecifier
      && ts.isStringLiteralLike(node.moduleSpecifier)
    ) {
      references.push({ specifier: node.moduleSpecifier.text, position: node.moduleSpecifier.getStart(sourceFile) });
    } else if (
      ts.isCallExpression(node)
      && node.expression.kind === ts.SyntaxKind.ImportKeyword
      && node.arguments.length === 1
      && ts.isStringLiteralLike(node.arguments[0])
    ) {
      references.push({ specifier: node.arguments[0].text, position: node.arguments[0].getStart(sourceFile) });
    }
    ts.forEachChild(node, visit);
  }
}

function resolveSourceFile(importer, specifier, files) {
  const basePath = path.resolve(path.dirname(importer), specifier);
  const candidates = [
    basePath,
    ...SOURCE_EXTENSIONS.map((extension) => `${basePath}${extension}`),
    ...SOURCE_EXTENSIONS.map((extension) => path.join(basePath, `index${extension}`))
  ];
  return candidates.find((candidate) => files.has(candidate)) ?? null;
}

function findCycles(importGraph) {
  const state = new Map();
  const stack = [];
  const stackIndex = new Map();
  const cycles = [];
  const cycleKeys = new Set();

  for (const file of importGraph.keys()) {
    if (!state.has(file)) visit(file);
  }
  return cycles;

  function visit(file) {
    state.set(file, "visiting");
    stackIndex.set(file, stack.length);
    stack.push(file);

    for (const target of importGraph.get(file) ?? []) {
      if (!state.has(target)) {
        visit(target);
      } else if (state.get(target) === "visiting") {
        const cycle = [...stack.slice(stackIndex.get(target)), target];
        const key = canonicalCycleKey(cycle);
        if (!cycleKeys.has(key)) {
          cycleKeys.add(key);
          cycles.push(cycle);
        }
      }
    }

    stack.pop();
    stackIndex.delete(file);
    state.set(file, "visited");
  }
}

function canonicalCycleKey(cycle) {
  const nodes = cycle.slice(0, -1).map(relative);
  const rotations = nodes.map((_, index) => [...nodes.slice(index), ...nodes.slice(0, index)].join("|"));
  return rotations.sort()[0];
}
