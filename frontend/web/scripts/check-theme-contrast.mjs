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
  const surfaces = ["--ng-bg", "--ng-surface", "--ng-editor", "--ng-panel"];
  for (const surface of surfaces) {
    for (const text of ["--ng-text", "--ng-muted", "--ng-faint", "--ng-primary", "--ng-success", "--ng-warning", "--ng-danger"]) {
      check(themeName, text, surface, theme[text], theme[surface], 4.5);
    }
    check(themeName, "--ng-focus-ring", surface, theme["--ng-focus-ring"], theme[surface], 3);
    check(themeName, "--ng-active-border", surface, theme["--ng-active-border"], theme[surface], 3);
    for (const overlay of ["--ng-selection", "--ng-active-surface", "--ng-hover"]) {
      for (const text of ["--ng-text", "--ng-muted", "--ng-faint"]) {
        checkCompositeBackground(themeName, text, overlay, surface, theme[text], theme[overlay], theme[surface], 4.5);
      }
    }
  }
  check(themeName, "--ng-primary-contrast", "--ng-primary", theme["--ng-primary-contrast"], theme["--ng-primary"], 4.5);
  check(themeName, "--ng-border-strong", "--ng-surface", theme["--ng-border-strong"], theme["--ng-surface"], 3);
  check(themeName, "--ng-google-text", "--ng-google-bg", theme["--ng-google-text"], theme["--ng-google-bg"], 4.5);
  check(themeName, "--ng-google-text", "--ng-google-hover", theme["--ng-google-text"], theme["--ng-google-hover"], 4.5);
  check(themeName, "--ng-google-border", "--ng-google-bg", theme["--ng-google-border"], theme["--ng-google-bg"], 3);
  check(themeName, "--ng-google-border", "--ng-surface", theme["--ng-google-border"], theme["--ng-surface"], 3);
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exitCode = 1;
} else {
  console.log("Theme token contrast smoke checks passed for light and dark modes.");
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
    [...block.matchAll(/(--ng-[\w-]+):\s*(#[0-9a-f]{6}|rgba?\([^)]+\))\s*;/gi)].map((match) => [match[1], match[2]])
  );
}

function check(themeName, foregroundName, backgroundName, foreground, background, minimum) {
  const actual = contrast(parseSolidColor(themeName, foregroundName, foreground), parseSolidColor(themeName, backgroundName, background));
  if (actual < minimum) {
    failures.push(
      `${themeName}: ${foregroundName} on ${backgroundName} is ${actual.toFixed(2)}:1; expected at least ${minimum}:1`
    );
  }
}

function checkCompositeBackground(themeName, foregroundName, overlayName, surfaceName, foreground, overlay, surface, minimum) {
  const resolvedSurface = parseSolidColor(themeName, surfaceName, surface);
  const resolvedOverlay = composite(parseColor(themeName, overlayName, overlay), resolvedSurface);
  const actual = contrast(parseSolidColor(themeName, foregroundName, foreground), resolvedOverlay);
  if (actual < minimum) {
    failures.push(
      `${themeName}: ${foregroundName} on ${overlayName} over ${surfaceName} is ${actual.toFixed(2)}:1; expected at least ${minimum}:1`
    );
  }
}

function contrast(first, second) {
  const lighter = Math.max(luminance(first), luminance(second));
  const darker = Math.min(luminance(first), luminance(second));
  return (lighter + 0.05) / (darker + 0.05);
}

function parseSolidColor(themeName, colorName, value) {
  const color = parseColor(themeName, colorName, value);
  if (color.alpha !== 1) throw new Error(`Expected solid color for ${themeName} ${colorName}`);
  return color;
}

function parseColor(themeName, colorName, value) {
  if (!value) throw new Error(`Missing color for ${themeName} ${colorName}`);
  if (value.startsWith("#")) {
    const channels = value.match(/[0-9a-f]{2}/gi).map((channel) => Number.parseInt(channel, 16));
    return { red: channels[0], green: channels[1], blue: channels[2], alpha: 1 };
  }
  const match = value.match(/^rgba?\(([^)]+)\)$/i);
  if (!match) throw new Error(`Unsupported color for ${themeName} ${colorName}: ${value}`);
  const channels = match[1].split(",").map((channel) => Number.parseFloat(channel.trim()));
  return {
    red: channels[0],
    green: channels[1],
    blue: channels[2],
    alpha: channels[3] ?? 1
  };
}

function composite(foreground, background) {
  return {
    red: foreground.red * foreground.alpha + background.red * (1 - foreground.alpha),
    green: foreground.green * foreground.alpha + background.green * (1 - foreground.alpha),
    blue: foreground.blue * foreground.alpha + background.blue * (1 - foreground.alpha),
    alpha: 1
  };
}

function luminance(color) {
  const channels = [color.red, color.green, color.blue].map((value) => value / 255);
  const [red, green, blue] = channels.map((value) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}
