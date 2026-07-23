import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const themePath = fileURLToPath(new URL("../src/design/theme.css", import.meta.url));
const css = readFileSync(themePath, "utf8");

const themes = {
  light: readVariables(readBlock(css, ":root {")),
  dark: readVariables(readBlock(css, ':root[data-theme="dark"] {'))
};

const failures = [];

for (const [themeName, theme] of Object.entries(themes)) {
  for (const surface of ["--ng-bg", "--ng-surface", "--ng-editor", "--ng-panel"]) {
    for (const text of ["--ng-text", "--ng-muted", "--ng-faint", "--ng-primary", "--ng-success", "--ng-warning", "--ng-danger"]) {
      check(themeName, text, surface, theme[text], theme[surface], 4.5);
    }
  }
  check(themeName, "--ng-primary-contrast", "--ng-primary", theme["--ng-primary-contrast"], theme["--ng-primary"], 4.5);
  check(themeName, "--ng-border-strong", "--ng-surface", theme["--ng-border-strong"], theme["--ng-surface"], 3);
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exitCode = 1;
} else {
  console.log("Theme contrast checks passed for light and dark modes.");
}

function readBlock(source, start) {
  const startIndex = source.indexOf(start);
  if (startIndex < 0) throw new Error(`Missing CSS block: ${start}`);
  const bodyStart = startIndex + start.length;
  const bodyEnd = source.indexOf("}", bodyStart);
  if (bodyEnd < 0) throw new Error(`Unclosed CSS block: ${start}`);
  return source.slice(bodyStart, bodyEnd);
}

function readVariables(block) {
  return Object.fromEntries(
    [...block.matchAll(/(--ng-[\w-]+):\s*(#[0-9a-f]{6})\s*;/gi)].map((match) => [match[1], match[2]])
  );
}

function check(themeName, foregroundName, backgroundName, foreground, background, minimum) {
  if (!foreground || !background) throw new Error(`Missing solid color for ${themeName} ${foregroundName} or ${backgroundName}`);
  const actual = contrast(foreground, background);
  if (actual < minimum) {
    failures.push(
      `${themeName}: ${foregroundName} on ${backgroundName} is ${actual.toFixed(2)}:1; expected at least ${minimum}:1`
    );
  }
}

function contrast(first, second) {
  const lighter = Math.max(luminance(first), luminance(second));
  const darker = Math.min(luminance(first), luminance(second));
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(hex) {
  const channels = hex.match(/[0-9a-f]{2}/gi).map((value) => Number.parseInt(value, 16) / 255);
  const [red, green, blue] = channels.map((value) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}
