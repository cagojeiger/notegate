import { appendFileSync, writeFileSync } from "node:fs";
import { test } from "@playwright/test";

import { runWorkbenchLive, workbenchContent, workbenchFixture } from "../e2e/support/workbench-live";
import { instrument, writeReport, type Measurement } from "./measure";

test("authenticated workbench performance baseline", async ({ browser, playwright, baseURL }) => {
  const measurements: Measurement[] = [];
  await runWorkbenchLive({ browser, playwright, baseURL }, {
    trials: 3,
    observePage: async (page, trial) => (await instrument(page, trial, measurements)).measure,
    report: ({ completed, textRevalidations, conditionalProbe, smallBodyProbes }) => {
      writeReport(measurements, { ...workbenchFixture, trials: 3, documentBytes: Buffer.byteLength(workbenchContent) }, completed);
      const directory = "test-results/browser-performance";
      writeFileSync(`${directory}/text-revalidation.json`, JSON.stringify({ completed, measurements: textRevalidations, conditionalProbe, smallBodyProbes }, null, 2));
      appendFileSync(`${directory}/summary.md`, [
        "", "## Unchanged text revalidation", "",
        "Stale reopen after 5,500ms outside the measured journey. Exact text GET only; no mocked network. Revalidation ends at responseEnd, not paint.",
        "Encoded/decoded sizes are response-body bytes; transfer includes browser-reported overhead. No URLs, identifiers, content hashes, bodies, or headers are exported.", "",
        "| Trial | GET starts | Conditional | Unchanged | Full body | Content returned | Updated at/by present | Cache body correct | Revalidation ms | Encoded B | Decoded B | Transfer B |",
        "|---|---:|---|---|---|---|---|---|---:|---:|---:|---:|",
        ...textRevalidations.map((row) => `| ${row.trial} | ${row.getStarts} | ${row.conditional} | ${row.unchanged} | ${row.fullBody} | ${row.contentReturned ?? "absent"} | ${row.updatedAtPresent}/${row.updatedByPresent} | ${row.cacheBodyCorrect} | ${row.revalidationMs.toFixed(1)} | ${row.encodedBodyBytes} | ${row.decodedBodyBytes} | ${row.transferBytes} |`),
        "", "Separate APIRequestContext probe: JSON response bytes, not browser wire bytes.", "",
        "| JSON bytes | Unchanged | Full body | Content returned | Updated at/by present |",
        "|---:|---|---|---|---|",
        ...(conditionalProbe ? [`| ${conditionalProbe.jsonBodyBytes} | ${conditionalProbe.unchanged} | ${conditionalProbe.fullBody} | ${conditionalProbe.contentReturned} | ${conditionalProbe.updatedAtPresent}/${conditionalProbe.updatedByPresent} |`] : []), "",
        "Small-body API probes run after all UI journeys. JSON body plus query-string comparison only; excludes headers, compression, TLS, and packet overhead.", "",
        "| Content B | Full JSON B | Conditional JSON B | Extra query B | Response plus query saved B |",
        "|---:|---:|---:|---:|---:|",
        ...smallBodyProbes.map((row) => `| ${row.contentBytes} | ${row.fullJsonBytes} | ${row.conditionalJsonBytes} | ${row.extraQueryBytes} | ${row.responsePlusQuerySavedBytes} |`), ""
      ].join("\n"));
    }
  });
});
