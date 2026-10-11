// Bounded HTTP regression workload against the existing disposable CI app/DB.
// These debug-build timings do not establish production capacity or user counts.
import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { setTimeout as sleep } from "node:timers/promises";

const base = "http://127.0.0.1:9191";
const origin = "http://localhost:9191";
const workers = 10;
const prefix = `ci-load-${randomUUID()}`;
const payload = "# Synthetic CI document\n" + "Only generated test content belongs here.\n".repeat(128);
const apiKey = process.env.NOTEGATE_PUBLIC_V2_API_KEY;
const session = process.env.NOTEGATE_BROWSER_SESSION;
const requests = [];
const reports = [];
let requestId = 0;
let space;
let passed = false;

assert.equal(process.env.GITHUB_ACTIONS, "true", "Run this workload in GitHub CI only");
assert.ok(apiKey && session, "The CI seed identity is required");

const sha256 = (text) => createHash("sha256").update(text).digest("hex");

async function request(path, { method = "GET", body, statuses = [200] } = {}) {
  const api = path.startsWith("/api/v2/");
  const purpose = api ? `${prefix} request ${++requestId}` : undefined;
  const started = performance.now();
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
  const data = response.status === 204 ? null : await response.json();
  const result = { data, status: response.status, ms: performance.now() - started, purpose };
  if (api) requests.push({ ...result, method });
  assert.ok(statuses.includes(response.status), `${method} ${path}: unexpected HTTP ${response.status}`);
  return result;
}

async function read(document) {
  const result = await request(`/api/v2/spaces/${space.id}/text/${document.id}`);
  assert.equal(result.data.text.content, document.content, "Stored content must match the last acknowledged write");
  assert.equal(result.data.text.content_sha256, sha256(document.content));
  return result;
}

async function scenario(name, concurrency, iterations, operation) {
  const samples = [];
  const started = performance.now();
  // Join every worker even on failure before cleanup or the next scenario.
  const results = await Promise.allSettled(Array.from({ length: concurrency }, async (_, worker) => {
    for (let iteration = 0; iteration < iterations; iteration++) {
      const result = await operation(worker, iteration);
      samples.push(result.ms);
      // Bound offered load below the default limiter; never retry errors away.
      await sleep(100);
    }
  }));
  samples.sort((a, b) => a - b);
  const percentile = (p) => samples[Math.ceil(samples.length * p) - 1] ?? 0;
  const p95 = percentile(0.95);
  reports.push(`| ${name} | ${concurrency} | ${samples.length} | ${percentile(0.5).toFixed(1)} | ${p95.toFixed(1)} | ${(samples.length / ((performance.now() - started) / 1000)).toFixed(1)} |`);
  const failed = results.find((result) => result.status === "rejected");
  if (failed) throw failed.reason;
  // A deliberately loose smoke budget, not a production SLO or a microbenchmark.
  assert.ok(p95 < 2000, `${name}: p95 ${p95.toFixed(1)}ms exceeded the 2000ms CI smoke budget`);
}

async function allPages(path, field) {
  const items = [];
  let cursor;
  for (let page = 0; page < 20; page++) {
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
  await scenario("Sequential reads", 1, 20, () => read(documents[0]));
  await scenario("Concurrent reads", workers, 20, (worker) => read(documents[worker]));
  await scenario("Mixed: 80% read / 20% write", workers, 10, async (worker, iteration) => {
    const document = documents[worker];
    if (iteration % 5 !== 4) return read(document);
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
  await scenario("Same-version write race", workers, 1, async (worker) => {
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
  assert.equal(conflicts, workers - 1);
  assert.ok(winner);
  shared.content = winner;
  for (const document of documents) await read(document);

  const history = (await allPages("/api/v1/me/command-invocations?surface=api", "command_invocations"))
    .filter((row) => row.purpose?.startsWith(prefix));
  assert.equal(history.length, requests.length, "Every completed API request must have one history row");
  assert.equal(new Set(history.map((row) => row.invocation_id)).size, history.length);
  const changes = (await allPages(`/api/v1/spaces/${space.id}/file-change-events`, "events"))
    .filter((row) => row.metadata.purpose?.startsWith(prefix));
  assert.equal(changes.length, requests.filter((row) => row.method !== "GET" && row.status < 400).length,
    "Every committed write must create one Change without orphan or duplicate events");
  for (const expected of requests) {
    const matches = history.filter((row) => row.purpose === expected.purpose);
    assert.equal(matches.length, 1, "History must not be missing or duplicated");
    const [row] = matches;
    assert.ok(row.invocation_id);
    assert.equal(row.actor_account_id, me.account.id);
    assert.equal(row.response.status, expected.status);
    assert.equal(row.outcome, expected.status < 400 ? "success" : "error");
    const linked = changes.filter((change) => change.metadata.invocation_id === row.invocation_id);
    const committed = expected.method !== "GET" && expected.status < 400;
    assert.equal(linked.length, committed ? 1 : 0, "Only committed writes may create Changes");
    if (committed) assert.equal(linked[0].metadata.purpose, expected.purpose);
  }
  passed = true;
} finally {
  const summary = [
    "### API concurrency smoke",
    "",
    "Disposable GitHub runner, debug build, one owner/Space/API key; 10 workers, not 1,000 users. HTTP timings include response-body consumption. RPS includes 100ms pacing per worker.",
    "",
    "| Scenario | Workers | Completed | p50 ms | p95 ms | Observed RPS |",
    "| --- | ---: | ---: | ---: | ---: | ---: |",
    ...reports,
    "",
    `Correctness and latency budget: **${passed ? "PASS" : "FAIL"}**. Gate: expected HTTP statuses, content/hash, one successful racing write, complete invocation/Changes linkage, scenario p95 < 2000ms; each HTTP request times out after 10s.`,
    "Small timing differences are diagnostic. This does not measure production capacity, multi-user isolation or a production SLO.",
    "",
  ].join("\n");
  console.log(summary);
  // The whole database is discarded with the job; hide the fixture from later UI tests.
  if (space) await request(`/api/v1/spaces/${space.id}`, { method: "DELETE", statuses: [204] });
}
