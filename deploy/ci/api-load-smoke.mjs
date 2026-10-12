// Bounded HTTP regression workload against the existing disposable CI app/DB.
// These debug-build timings do not establish production capacity or user counts.
import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { setTimeout as sleep } from "node:timers/promises";

const base = "http://127.0.0.1:9191";
const origin = "http://localhost:9191";
const workers = 100;
const raceWorkers = 10;
const prefix = `ci-load-${randomUUID()}`;
const payload = "# Synthetic CI document\n" + "Only generated test content belongs here.\n".repeat(128);
const apiKey = process.env.NOTEGATE_PUBLIC_V2_API_KEY;
const session = process.env.NOTEGATE_BROWSER_SESSION;
const requests = [];
const reports = [];
let requestId = 0;
let inFlight = 0;
let peakInFlight = 0;
let space;
let passed = false;

assert.equal(process.env.GITHUB_ACTIONS, "true", "Run this workload in GitHub CI only");
assert.ok(apiKey && session, "The CI seed identity is required");

const sha256 = (text) => createHash("sha256").update(text).digest("hex");

async function request(path, { method = "GET", body, statuses = [200] } = {}) {
  const api = path.startsWith("/api/v2/");
  const purpose = api ? `${prefix} request ${++requestId}` : undefined;
  const started = performance.now();
  let status = 0;
  let data;
  let ok = false;
  let ms;
  peakInFlight = Math.max(peakInFlight, ++inFlight);
  try {
    const response = await fetch(`${base}${path}`, {
      method,
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
      headers: {
        "Content-Type": "application/json",
        ...(api
          ? { Authorization: `Bearer ${apiKey}`, "X-Notegate-Purpose": encodeURIComponent(purpose) }
          : { Cookie: `notegate_browser_session=${session}`, Origin: origin }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    status = response.status;
    data = status === 204 ? null : await response.json();
    ok = statuses.includes(status);
  } finally {
    ms = performance.now() - started;
    inFlight--;
    // Keep measurements on failure without retaining thousands of response bodies.
    if (api) requests.push({ status, method, purpose, ms, ok });
  }
  assert.ok(ok, `${method} ${path}: unexpected HTTP ${status}`);
  return { data, status, ms, purpose };
}

async function read(document) {
  const result = await request(`/api/v2/spaces/${space.id}/text/${document.id}`);
  assert.equal(result.data.text.content, document.content, "Stored content must match the last acknowledged write");
  assert.equal(result.data.text.content_sha256, sha256(document.content));
  return result;
}

async function scenario(name, concurrency, iterations, pauseMs, operation) {
  const firstRequest = requests.length;
  const started = performance.now();
  peakInFlight = 0;
  // Join every worker even on failure before cleanup or the next scenario.
  const results = await Promise.allSettled(Array.from({ length: concurrency }, async (_, worker) => {
    for (let iteration = 0; iteration < iterations; iteration++) {
      await operation(worker, iteration);
      // Bound offered load below the default limiter; never retry errors away.
      await sleep(pauseMs);
    }
  }));
  const measured = requests.slice(firstRequest);
  const samples = measured.map((row) => row.ms).sort((a, b) => a - b);
  const percentile = (p) => samples[Math.ceil(samples.length * p) - 1] ?? 0;
  const p95 = percentile(0.95);
  const seconds = (performance.now() - started) / 1000;
  const errors = measured.filter((row) => !row.ok).length;
  const rateLimited = measured.filter((row) => row.status === 429).length;
  reports.push(`| ${name} | ${concurrency} | ${peakInFlight} | ${samples.length}/${concurrency * iterations} | ${seconds.toFixed(1)} | ${percentile(0.5).toFixed(1)} | ${p95.toFixed(1)} | ${percentile(0.99).toFixed(1)} | ${(samples.length / seconds).toFixed(1)} | ${errors} (${(100 * errors / (samples.length || 1)).toFixed(1)}%) | ${rateLimited} |`);
  const failed = results.find((result) => result.status === "rejected");
  if (failed) throw failed.reason;
  // A deliberately loose smoke budget, not a production SLO or a microbenchmark.
  assert.ok(p95 < 2000, `${name}: p95 ${p95.toFixed(1)}ms exceeded the 2000ms CI smoke budget`);
}

async function allPages(path, field) {
  const items = [];
  let cursor;
  // Account for this workload plus the small pre-existing API smoke fixture.
  const maxPages = Math.ceil(requests.length / 100) + 2;
  for (let page = 0; page < maxPages; page++) {
    const query = new URLSearchParams({ limit: "100", ...(cursor ? { cursor } : {}) });
    const { data } = await request(`${path}${path.includes("?") ? "&" : "?"}${query}`);
    items.push(...data[field]);
    if (!data.page.has_more) return items;
    assert.ok(data.page.next_cursor && data.page.next_cursor !== cursor, "Pagination must advance");
    cursor = data.page.next_cursor;
  }
  throw new Error("CI fixture exceeded the bounded history pagination workload");
}

try {
  const { data: me } = await request("/api/v2/me");
  ({ data: space } = await request("/api/v1/spaces", {
    method: "POST", body: { name: prefix }, statuses: [201],
  }));
  await request(`/api/v1/spaces/${space.id}`, {
    method: "PATCH", body: { default_external_access_enabled: true },
  });
  await request(`/api/v1/spaces/${space.id}/agents/${me.account.id}`, {
    method: "PUT", body: { permission: "write" },
  });

  const documents = [];
  for (let worker = 0; worker <= workers; worker++) {
    const content = `${payload}worker ${worker}\n`;
    const { data } = await request(`/api/v2/spaces/${space.id}/nodes`, {
      method: "POST", statuses: [201],
      body: { parent_id: space.root_node_id, name: `${worker}.md`, kind: "text", content },
    });
    documents.push({ id: data.id, content });
  }
  await read(documents[0]); // Warm the route before reporting.
  await scenario("Sequential reads (100ms pause)", 1, 20, 100, () => read(documents[0]));
  await scenario("10 readers (100ms pause)", 10, 20, 100, (worker) => read(documents[worker]));
  await scenario("100 readers (1s pause)", workers, 20, 1000, (worker) => read(documents[worker]));
  await scenario("100 mixed: 80% read / 20% write (1s pause)", workers, 20, 1000, async (worker, iteration) => {
    const document = documents[worker];
    // Spread writes across workers instead of synchronizing every fifth round.
    if ((worker + iteration) % 5 !== 4) return read(document);
    const content = `${payload}worker ${worker} iteration ${iteration}\n`;
    const result = await request(`/api/v2/spaces/${space.id}/text/${document.id}`, {
      method: "PUT", body: { content, expected_sha256: sha256(document.content) },
    });
    assert.equal(result.data.content_sha256, sha256(content));
    document.content = content;
    return result;
  });

  const shared = documents[workers];
  const originalHash = sha256(shared.content);
  let winner;
  let conflicts = 0;
  await scenario("Same-version write race", raceWorkers, 1, 0, async (worker) => {
    const content = `${payload}contender ${worker}\n`;
    const result = await request(`/api/v2/spaces/${space.id}/text/${shared.id}`, {
      method: "PUT", statuses: [200, 409],
      body: { content, expected_sha256: originalHash },
    });
    if (result.status === 200) {
      assert.equal(winner, undefined, "Exactly one writer may commit with the same expected hash");
      assert.equal(result.data.content_sha256, sha256(content));
      winner = content;
    } else {
      conflicts++;
    }
    return result;
  });
  assert.equal(conflicts, raceWorkers - 1);
  assert.ok(winner);
  shared.content = winner;
  for (const document of documents) await read(document);

  const history = (await allPages("/api/v1/me/command-invocations?surface=api", "command_invocations"))
    .filter((row) => row.purpose?.startsWith(prefix));
  assert.equal(history.length, requests.length, "Every completed API request must have one history row");
  assert.equal(new Set(history.map((row) => row.invocation_id)).size, history.length);
  const historyByPurpose = new Map(history.map((row) => [row.purpose, row]));
  assert.equal(historyByPurpose.size, history.length, "History purposes must be unique");
  const changes = (await allPages(`/api/v1/spaces/${space.id}/file-change-events`, "events"))
    .filter((row) => row.metadata.purpose?.startsWith(prefix));
  assert.equal(changes.length, requests.filter((row) => row.method !== "GET" && row.status < 400).length,
    "Every committed write must create one Change without orphan or duplicate events");
  const changesByInvocation = new Map(changes.map((row) => [row.metadata.invocation_id, row]));
  assert.equal(changesByInvocation.size, changes.length, "A committed write must not duplicate Changes");
  for (const expected of requests) {
    const row = historyByPurpose.get(expected.purpose);
    assert.ok(row, "History must not be missing");
    assert.ok(row.invocation_id);
    assert.equal(row.actor_account_id, me.account.id);
    assert.equal(row.response.status, expected.status);
    assert.equal(row.outcome, expected.status < 400 ? "success" : "error");
    const linked = changesByInvocation.get(row.invocation_id);
    const committed = expected.method !== "GET" && expected.status < 400;
    assert.equal(Boolean(linked), committed, "Only committed writes may create Changes");
    if (committed) assert.equal(linked.metadata.purpose, expected.purpose);
  }
  passed = true;
} finally {
  const summary = [
    "### API concurrency smoke",
    "",
    "Disposable GitHub runner, debug build, one owner/Space/API key; up to 100 virtual workers, not 100 independent accounts. The load generator, app and PostgreSQL share the runner. HTTP timings include response-body consumption.",
    "",
    "| Scenario | Workers | Peak in-flight | Attempted/planned | Seconds | p50 ms | p95 ms | p99 ms | Observed RPS | Request errors | 429 |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ...reports,
    "",
    "RPS includes response wait and per-worker pacing; this is a closed-loop workload, not a maximum-capacity benchmark. Peak in-flight is measured at the client. Errors count unexpected HTTP statuses or transport/parse failures; expected 409 conflicts are excluded. A failed worker stops without retry; content/history assertions are separate correctness gates.",
    "",
    `API attempts including setup/readback: ${requests.length}. Successful writes: ${requests.filter((row) => row.ok && row.method !== "GET").length}.`,
    "",
    `Correctness and latency budget: **${passed ? "PASS" : "FAIL"}**. Gate: expected HTTP statuses, content/hash, one successful racing write, complete invocation/Changes linkage, scenario p95 < 2000ms; each HTTP request times out after 10s.`,
    "Small timing differences are diagnostic. This does not measure production capacity, multi-user isolation or a production SLO.",
    "",
  ].join("\n");
  console.log(summary);
  // The whole database is discarded with the job; hide the fixture from later UI tests.
  if (space) await request(`/api/v1/spaces/${space.id}`, { method: "DELETE", statuses: [204] });
}
