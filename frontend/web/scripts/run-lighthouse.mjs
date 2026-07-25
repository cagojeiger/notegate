import { spawnSync } from "node:child_process";

import { chromium } from "@playwright/test";

const command = process.platform === "win32" ? "lhci.cmd" : "lhci";
const result = spawnSync(command, ["autorun", "--config=./lighthouserc.cjs"], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    CHROME_PATH: process.env.CHROME_PATH || chromium.executablePath()
  },
  stdio: "inherit"
});

if (result.error) throw result.error;
process.exit(result.status ?? 1);
