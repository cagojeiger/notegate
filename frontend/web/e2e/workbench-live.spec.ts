import { test } from "@playwright/test";

import { runWorkbenchLive } from "./support/workbench-live";

test("live workbench preserves cached text, saves, history and trash pagination", async ({ browser, playwright, baseURL }) => {
  await runWorkbenchLive({ browser, playwright, baseURL });
});
