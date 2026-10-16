// Bounded attack scenarios for the disposable CI application, never production.
import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

const base = "http://127.0.0.1:9191";
const version = readFileSync("VERSION", "utf8").trim();
const prefix = `dast-${randomUUID()}`;
const requests = [];
const scenarios = [];
let activeScenario = "setup";
let rpcId = 0;
let setupComplete = false;
const sha256 = (text) => createHash("sha256").update(text).digest("hex");
const check = (condition, message) => { if (!condition) throw new Error(message); };

function identity(suffix) {
  const session = process.env[`NOTEGATE_DAST_BROWSER_SESSION_${suffix}`];
  const key = process.env[`NOTEGATE_DAST_API_KEY_${suffix}`];
  const account = process.env[`NOTEGATE_DAST_ACCOUNT_ID_${suffix}`];
  check(session && key && account, "Two independent CI seed identities are required");
  return { session, key, account };
}

async function request(path, { actor, browser = false, method = "GET", body, statuses = [200], headers = {} } = {}) {
  const url = new URL(path, base);
  check(url.origin === base && url.pathname.startsWith("/"), "Only the fixed loopback CI target is allowed");
  // Sequential requests with a pause deliberately avoid load testing.
  await sleep(100);
  const started = performance.now();
  const measurement = { scenario: activeScenario, method, path: url.pathname, status: 0, elapsed_ms: 0 };
  requests.push(measurement);
  try {
    const response = await fetch(url, {
      method,
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
      headers: {
        "Content-Type": "application/json",
        ...(actor ? browser
          ? { Cookie: `notegate_browser_session=${actor.session}`, Origin: base }
          : { Authorization: `Bearer ${actor.key}`, "X-Notegate-Purpose": "Synthetic CI security scenario" }
          : {}),
        ...headers,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    measurement.status = response.status;
    const text = await response.text();
    check(statuses.includes(response.status), `${method} ${url.pathname}: unexpected HTTP ${response.status}`);
    let data = null;
    if (text) {
      try { data = JSON.parse(text); } catch { throw new Error(`${method} ${url.pathname}: expected JSON response`); }
    }
    return { data, status: response.status, text };
  } catch (error) {
    // Fetch errors can include headers or response bodies in third-party implementations.
    if (error instanceof TypeError || error?.name === "TimeoutError") {
      throw new Error(`${method} ${url.pathname}: transport failure`);
    }
    throw error;
  } finally {
    measurement.elapsed_ms = Number((performance.now() - started).toFixed(1));
  }
}

async function scenario(name, run) {
  activeScenario = name;
  const start = requests.length;
  const started = performance.now();
  const report = { name, status: "passed" };
  try { await run(); } catch (error) {
    report.status = "failed";
    report.failure = error instanceof Error ? error.message : "Scenario assertion failed";
    process.exitCode = 1;
  }
  report.requests = requests.length - start;
  report.elapsed_ms = Number((performance.now() - started).toFixed(1));
  scenarios.push(report);
}

function noCanary(result, fixture) {
  check(!result.text.includes(fixture.canary), "Denied response exposed the other owner's synthetic document body");
}

async function command(surface, actor, tool, input, deniedKind) {
  const mcp = surface === "mcp";
  const result = await request(mcp ? "/mcp/v2" : "/cli", {
    actor,
    method: "POST",
    headers: mcp
      ? { Accept: "application/json, text/event-stream", "MCP-Protocol-Version": "2026-07-28", "Mcp-Method": "tools/call", "Mcp-Name": tool }
      : { "X-Notegate-Cli-Version": version, "X-Notegate-Command-Protocol": "1" },
    body: mcp
      ? { jsonrpc: "2.0", id: ++rpcId, method: "tools/call", params: { name: tool, arguments: input,
        _meta: { "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} } } }
      : { tool, input },
    statuses: deniedKind ? [mcp ? deniedKind === "not_found" ? 400 : 200 : deniedKind === "not_found" ? 404 : 403] : [200],
  });
  if (deniedKind) {
    const failure = mcp ? result.data?.error : result.data;
    check(failure?.data?.kind === deniedKind, `${surface}: expected structured ${deniedKind} error`);
    check(!mcp || !result.data?.result, "MCP denial must not contain a successful result");
    return result;
  }
  check(!result.data?.error, `${surface}: positive control returned an application error`);
  if (!mcp) return { ...result, data: result.data };
  const output = result.data?.result;
  check(output && !output.isError, "MCP positive control must succeed");
  let data = output.structuredContent;
  if (!data) {
    const text = output.content?.find((part) => part.type === "text")?.text;
    try { data = JSON.parse(text); } catch { throw new Error("MCP positive control did not return structured JSON"); }
  }
  return { ...result, data };
}

async function readFixture(actor, fixture, browser = false) {
  const result = await request(`/api/v${browser ? 1 : 2}/spaces/${fixture.space.id}/text/${fixture.node.id}`, { actor, browser });
  check(result.data?.text?.content === fixture.content, "Positive control document body must match the synthetic fixture");
  check(result.data?.text?.content_sha256 === sha256(fixture.content), "Stored body hash changed unexpectedly");
  return result;
}

async function children(actor, fixture) {
  const { data } = await request(`/api/v2/spaces/${fixture.space.id}/nodes/${fixture.space.root_node_id}/children`, { actor });
  check(Array.isArray(data.children) && !data.page?.has_more, "Fixture children must fit one complete page");
  return data.children.map((node) => `${node.id}:${node.name}`).sort();
}

async function unchanged(actor, fixture, before) {
  await readFixture(actor, fixture);
  const after = await children(actor, fixture);
  check(JSON.stringify(before) === JSON.stringify(after), "Denied mutation changed the owner's children");
}

async function setupFixture(actor, label) {
  const { data: user } = await request("/api/v1/me", { actor, browser: true });
  check(user?.account?.id === actor.account && user.account.kind === "user", "Browser seed must resolve to its expected user");
  const { data: agent } = await request("/api/v2/me", { actor });
  check(agent?.account?.kind === "agent" && agent.account.id !== actor.account, "Agent seed must resolve to a distinct Agent account");
  const { data: agents } = await request("/api/v1/agents?limit=100", { actor, browser: true });
  check(agents.agents?.some((entry) => entry.id === agent.account.id && entry.owner_user_id === actor.account), "Agent must belong to the expected user");
  for (const surface of ["cli", "mcp"]) {
    const { data } = await command(surface, actor, "me", {});
    check(data?.account?.id === agent.account.id && data.account.kind === "agent", `${surface}: seed identity changed across transports`);
  }
  const name = `${prefix}-${label}`;
  const { data: space } = await request("/api/v1/spaces", { actor, browser: true, method: "POST", body: { name }, statuses: [201] });
  check(space.id && space.root_node_id, "Space positive control must return identifiers");
  await request(`/api/v1/spaces/${space.id}`, { actor, browser: true, method: "PATCH", body: { default_external_access_enabled: true } });
  await request(`/api/v1/spaces/${space.id}/agents/${agent.account.id}`, { actor, browser: true, method: "PUT", body: { permission: "write" } });
  const canary = `SYNTHETIC-PRIVATE-${randomUUID()}`;
  const historical = `${canary}\nOriginal synthetic version.\n`;
  const { data: node } = await request(`/api/v2/spaces/${space.id}/nodes`, {
    actor, method: "POST", statuses: [201],
    body: { parent_id: space.root_node_id, name: "private.md", kind: "text", content: historical },
  });
  check(node.id, "Document positive control must return an identifier");
  const content = `${canary}\nCurrent synthetic version.\n`;
  await request(`/api/v2/spaces/${space.id}/text/${node.id}`, { actor, method: "PUT", body: { content, expected_sha256: sha256(historical) } });
  const fixture = { space, node, content, historical, canary };
  await readFixture(actor, fixture);
  await readFixture(actor, fixture, true);
  const revisionPath = `/api/v1/spaces/${space.id}/text/${node.id}/revisions`;
  const { data: history } = await request(revisionPath, { actor, browser: true });
  const revision = history.revisions?.find((entry) => entry.content_sha256 === sha256(historical));
  check(revision?.id, "Historical version positive control must exist");
  const { data: old } = await request(`${revisionPath}/${revision.id}`, { actor, browser: true });
  check(old.content === historical, "Historical body must match its positive control");
  fixture.revision = revision;
  return fixture;
}

async function crossOwner(attacker, owner, fixture) {
  const before = await children(owner, fixture);
  const paths = [
    `/api/v1/spaces/${fixture.space.id}`,
    `/api/v1/spaces/${fixture.space.id}/nodes/${fixture.node.id}`,
    `/api/v1/spaces/${fixture.space.id}/text/${fixture.node.id}`,
    `/api/v1/spaces/${fixture.space.id}/text/${fixture.node.id}/revisions`,
    `/api/v1/spaces/${fixture.space.id}/text/${fixture.node.id}/revisions/${fixture.revision.id}`,
    `/api/v2/spaces/${fixture.space.id}`,
    `/api/v2/spaces/${fixture.space.id}/nodes/${fixture.node.id}`,
    `/api/v2/spaces/${fixture.space.id}/text/${fixture.node.id}`,
    `/api/v2/spaces/${fixture.space.id}/nodes/${fixture.space.root_node_id}/children`,
  ];
  for (const path of paths) {
    noCanary(await request(path, { actor: attacker, browser: path.startsWith("/api/v1/"), statuses: [404] }), fixture);
  }
  for (const browser of [true, false]) {
    noCanary(await request(`/api/v${browser ? 1 : 2}/spaces/${fixture.space.id}/text/${fixture.node.id}`, {
      actor: attacker, browser, method: "PUT", statuses: [404],
      body: { content: "Synthetic unauthorized overwrite", expected_sha256: sha256(fixture.content) },
    }), fixture);
    await unchanged(owner, fixture, before);
  }
  noCanary(await request(`/api/v1/spaces/${fixture.space.id}/text/${fixture.node.id}/revisions/${fixture.revision.id}/restore`, {
    actor: attacker, browser: true, method: "POST", statuses: [404], body: { expected_sha256: sha256(fixture.content) },
  }), fixture);
  await unchanged(owner, fixture, before);
  for (const surface of ["cli", "mcp"]) {
    const target = `${fixture.space.name}:/private.md`;
    noCanary(await command(surface, attacker, "read", { purpose: "Synthetic cross-owner denial", op: "read", target }, "not_found"), fixture);
    noCanary(await command(surface, attacker, "write", { purpose: "Synthetic cross-owner denial", op: "write", target, content: "Synthetic unauthorized overwrite", expected_sha256: sha256(fixture.content) }, "not_found"), fixture);
    await unchanged(owner, fixture, before);
  }
}

async function readOnly(owner, fixture) {
  const { data: agent } = await request("/api/v1/agents", { actor: owner, browser: true, method: "POST", statuses: [201], body: { name: `${prefix}-read-only` } });
  const { data: key } = await request(`/api/v1/agents/${agent.id}/keys`, {
    actor: owner, browser: true, method: "POST", statuses: [201],
    body: { name: "synthetic-read-only", scopes: [], expires_at: new Date(Date.now() + 3_600_000).toISOString() },
  });
  check(key.token && key.id, "Read-only fixture requires a temporary Agent key");
  await request(`/api/v1/spaces/${fixture.space.id}/agents/${agent.id}`, { actor: owner, browser: true, method: "PUT", body: { permission: "read" } });
  const reader = { key: key.token };
  const { data: me } = await request("/api/v2/me", { actor: reader });
  check(me?.account?.id === agent.id, "Read-only key must resolve to the new Agent");
  await readFixture(reader, fixture);
  const before = await children(owner, fixture);
  await request(`/api/v2/spaces/${fixture.space.id}/text/${fixture.node.id}`, { actor: reader, method: "PUT", statuses: [403], body: { content: "Synthetic denied write", expected_sha256: sha256(fixture.content) } });
  await unchanged(owner, fixture, before);
  await request(`/api/v2/spaces/${fixture.space.id}/nodes`, { actor: reader, method: "POST", statuses: [403], body: { parent_id: fixture.space.root_node_id, name: "forbidden.md", kind: "text", content: "Synthetic denied create" } });
  await unchanged(owner, fixture, before);
  for (const surface of ["cli", "mcp"]) {
    const target = `${fixture.space.name}:/private.md`;
    const { data } = await command(surface, reader, "read", { purpose: "Synthetic read-only positive control", op: "read", target });
    check(data?.content === fixture.content, `${surface}: read-only positive control must read the fixture`);
    await command(surface, reader, "write", { purpose: "Synthetic read-only denial", op: "write", target, content: "Synthetic denied write", expected_sha256: sha256(fixture.content) }, "forbidden");
    await unchanged(owner, fixture, before);
    await command(surface, reader, "manage", { purpose: "Synthetic read-only create denial", op: "mkdir", target: `${fixture.space.name}:/forbidden-folder` }, "forbidden");
    await unchanged(owner, fixture, before);
  }
  await request(`/api/v1/agents/${agent.id}/keys/${key.id}`, { actor: owner, browser: true, method: "DELETE", statuses: [204] });
  await request("/api/v2/me", { actor: reader, statuses: [401] });
  await request("/cli", { actor: reader, method: "POST", statuses: [401], body: { tool: "me", input: {} }, headers: { "X-Notegate-Cli-Version": version, "X-Notegate-Command-Protocol": "1" } });
  await request("/mcp/v2", { actor: reader, method: "POST", statuses: [401], body: { jsonrpc: "2.0", id: ++rpcId, method: "tools/call", params: { name: "me", arguments: {} } }, headers: { Accept: "application/json, text/event-stream", "MCP-Protocol-Version": "2026-07-28" } });
}

async function trashOwnership(owner, attacker, fixture) {
  const canary = `SYNTHETIC-TRASH-PRIVATE-${randomUUID()}`;
  const content = `${canary}\n`;
  const name = `trash-${randomUUID()}.md`;
  const { data: node } = await request(`/api/v2/spaces/${fixture.space.id}/nodes`, {
    actor: owner, method: "POST", statuses: [201],
    body: { parent_id: fixture.space.root_node_id, name, kind: "text", content },
  });
  const path = `/api/v1/spaces/${fixture.space.id}/nodes/${node.id}`;
  const textPath = `/api/v1/spaces/${fixture.space.id}/text/${node.id}`;
  const { data: before } = await request(textPath, { actor: owner, browser: true });
  check(before?.text?.content === content, "Trash positive control must contain the synthetic body before deletion");
  await request(path, { actor: owner, browser: true, method: "DELETE", statuses: [204] });
  const list = async (actor) => {
    const { data } = await request("/api/v1/me/trash?limit=100", { actor, browser: true });
    check(Array.isArray(data.items) && !data.page?.has_more, "Synthetic trash entries must fit one complete page");
    return data.items;
  };
  // There is no trash-detail GET route: the owner-scoped list supplies detail metadata.
  const entry = (await list(owner)).find((item) => item.id === node.id);
  check(entry?.space_id === fixture.space.id && entry.name === name && entry.recoverable && !entry.deletion_pending && entry.deletion_operation_id, "Owner trash metadata must identify the recoverable deletion instance");
  const privateFields = [node.id, name, entry.path, entry.deletion_operation_id];
  const otherItems = await list(attacker);
  check(!privateFields.some((value) => JSON.stringify(otherItems).includes(value)), "Other owner's trash list exposed private item metadata");
  for (const actor of [owner, attacker]) {
    await request(textPath, { actor, browser: true, statuses: [404] });
  }
  const query = new URLSearchParams({ deleted_at: entry.deleted_at, deletion_operation_id: entry.deletion_operation_id });
  const trashPath = `/api/v1/me/trash/spaces/${fixture.space.id}/nodes/${node.id}`;
  for (const [method, suffix] of [["POST", "/restore"], ["DELETE", ""]]) {
    const denied = await request(`${trashPath}${suffix}?${query}`, { actor: attacker, browser: true, method, statuses: [404] });
    check(!denied.text.includes(canary) && !denied.text.includes(entry.deletion_operation_id), "Denied trash mutation exposed private content or deletion metadata");
    const current = (await list(owner)).find((item) => item.id === node.id);
    check(JSON.stringify(current) === JSON.stringify(entry), "Denied trash mutation changed the owner's deletion state");
    await request(textPath, { actor: owner, browser: true, statuses: [404] });
  }
  await request(`${trashPath}/restore?${query}`, { actor: owner, browser: true, method: "POST", statuses: [204] });
  const { data: restored } = await request(textPath, { actor: owner, browser: true });
  check(restored?.text?.content === content && restored.text.content_sha256 === sha256(content), "Owner must retain the ability to restore the unchanged private body");
  check(!(await list(owner)).some((item) => item.id === node.id), "Successful owner restore must remove the item from trash");
}

try {
  check(process.env.GITHUB_ACTIONS === "true", "Run security scenarios in GitHub CI only");
  const a = identity("A");
  const b = identity("B");
  check(a.account !== b.account && a.key !== b.key && a.session !== b.session, "CI identities must be independent");
  const first = await setupFixture(a, "a");
  const second = await setupFixture(b, "b");
  setupComplete = true;
  scenarios.push({ name: "Two owners, Agents, documents and historical versions: positive controls", status: "passed", requests: requests.length });
  await scenario("Anonymous access is rejected", async () => {
    for (const path of ["/api/v1/me", "/api/v2/me", "/cli", "/mcp/v2"]) {
      const result = await request(path, { method: path.startsWith("/api/") ? "GET" : "POST", body: path.startsWith("/api/") ? undefined : {}, statuses: [401] });
      noCanary(result, first);
    }
  });
  await scenario("Owner A cannot read or mutate owner B's data", () => crossOwner(a, b, second));
  await scenario("Owner B cannot read or mutate owner A's data", () => crossOwner(b, a, first));
  await scenario("Read-only Agent cannot gain write permission through V2, CLI or MCP", () => readOnly(a, first));
  await scenario("Owner B cannot discover, restore or purge owner A's trash", () => trashOwnership(a, b, first));
  await scenario("Owner A cannot discover, restore or purge owner B's trash", () => trashOwnership(b, a, second));
  // ZAP receives synthetic identifiers and request bodies only; credentials stay in env.
  const textUrl = `${base}/api/v2/spaces/${first.space.id}/text/${first.node.id}`;
  mkdirSync("security-results", { recursive: true });
  writeFileSync("security-results/fixture.json", JSON.stringify({
    space: { id: first.space.id, name: first.space.name, root_node_id: first.space.root_node_id },
    document: { id: first.node.id, name: "private.md", path: "/private.md", sha256: sha256(first.content) },
    requests: [
      { method: "GET", url: textUrl },
      { method: "GET", url: `${base}/api/v2/spaces/${first.space.id}/nodes/${first.space.root_node_id}/children` },
      { method: "PUT", url: textUrl, body: { content: first.content, expected_sha256: sha256(first.content) } },
    ],
  }, null, 2));
} catch (error) {
  process.exitCode = 1;
  scenarios.push({ name: "Fixture setup", status: "failed", failure: error instanceof Error ? error.message : "Fixture setup failed" });
} finally {
  mkdirSync("security-results", { recursive: true });
  writeFileSync("security-results/api.json", JSON.stringify({ target: base, setup_complete: setupComplete, scenarios, requests }, null, 2));
  const lines = ["# NoteGate isolated API attack scenarios", "", `Target: disposable CI app at ${base}. Requests are sequential with a 100 ms pause.`, "", "| Scenario | Result | Requests |", "| --- | --- | --- |", ...scenarios.map((entry) => `| ${entry.name} | ${entry.status} | ${entry.requests ?? 0} |`), "", "Fixtures contain synthetic data only. Tokens, cookies, request bodies and response bodies are excluded from the API report.", "No production, infrastructure perimeter, throughput or complete ASVS coverage is established by this pilot."];
  for (const entry of scenarios.filter((entry) => entry.status === "failed")) lines.push("", `Failure (${entry.name}): ${entry.failure}`);
  writeFileSync("security-results/summary-api.md", `${lines.join("\n")}\n`);
  console.log(`API security pilot: ${scenarios.filter((entry) => entry.status === "passed").length} passed, ${scenarios.filter((entry) => entry.status === "failed").length} failed; ${requests.length} requests. See security-results/summary-api.md.`);
}
