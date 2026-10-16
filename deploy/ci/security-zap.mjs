// CI-only pilot: explicit synthetic API requests, three active rules, no crawler.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { isIPv4 } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

assert.equal(process.env.GITHUB_ACTIONS, "true", "Run this workload in GitHub CI only");
const image = "ghcr.io/zaproxy/zaproxy@sha256:7aaa659b0d43078febd82e29bad112285c370727e86ab8340444220e17d9f0d2"; // 2.17.0, stable manifest verified 2026-10-16
const results = resolve(import.meta.dirname, "../../security-results");
const key = process.env.NOTEGATE_DAST_API_KEY_A;
assert.ok(key, "Disposable CI key is required");
mkdirSync(results, { recursive: true });
const work = mkdtempSync(join(tmpdir(), "notegate-zap-"));
const redact = (value) => String(value).replaceAll(key, "[REDACTED]").replaceAll(encodeURIComponent(key), "[REDACTED]");
const docker = (args) => {
  const result = spawnSync("docker", args, { encoding: "utf8", timeout: 120_000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, redact(result.stderr || result.error?.message || `docker ${args[0]} failed`));
  return result.stdout.trim();
};
const evidence = { commit: process.env.GITHUB_SHA, image, rulesRequested: [6, 40012, 40018],
  scope: "disposable API v2 fixture only; scanner internet egress blocked", authenticatedFixture: false,
  activeRequestsGenerated: false, completed: false, alerts: [] };

try {
  const fixture = JSON.parse(readFileSync(join(results, "fixture.json"), "utf8"));
  assert.ok(fixture.space?.id && fixture.document?.id && fixture.requests?.length > 0, "Missing successful API fixture");
  for (const id of [fixture.space.id, fixture.document.id]) assert.match(id, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i);
  assert.match(fixture.document.sha256, /^[a-f0-9]{64}$/);

  docker(["pull", image]);
  docker(["network", "create", "--internal", "notegate-dast"]);
  const gateway = docker(["network", "inspect", "notegate-dast", "--format", "{{(index .IPAM.Config 0).Gateway}}"]);
  assert.ok(isIPv4(gateway), "Expected a Docker internal bridge gateway");
  const target = `http://${gateway}:9191`;
  const apiPrefix = `/api/v2/spaces/${fixture.space.id}/`;
  const documentUrl = `${target}${apiPrefix}text/${fixture.document.id}`;
  const documentTests = (url, hash) => [
    { name: "Scanner read the synthetic document", type: "url", url, operator: "or",
      responseBodyRegex: `(?s).*"id"\\s*:\\s*"${fixture.document.id}".*`, onFail: "error" },
    ...(hash ? [{ name: "Scanner read the expected document hash", type: "url", url, operator: "or",
      responseBodyRegex: `(?s).*"content_sha256"\\s*:\\s*"${fixture.document.sha256}".*`, onFail: "error" }] : [])
  ];
  const requests = fixture.requests.map(({ method, url, body }) => {
    const parsed = new URL(url);
    assert.equal(parsed.origin, "http://127.0.0.1:9191", "Refuse non-loopback fixture targets");
    assert.ok(parsed.pathname.startsWith(apiPrefix), "Refuse targets outside the synthetic Space");
    assert.ok(["GET", "PUT"].includes(method), "Only seeded read and synthetic text-update requests are scanned");
    return { name: `${method} ${parsed.pathname}`, url: `${target}${parsed.pathname}${parsed.search}`, method,
      headers: ["Content-Type: application/json"], ...(body === undefined ? {} : { data: JSON.stringify(body) }), responseCode: 200 };
  });
  const plan = {
    env: { contexts: [{ name: "notegate", urls: [`${target}${apiPrefix}`] }],
      parameters: { failOnError: true, failOnWarning: true, continueOnFailure: false, progressToStdout: true } },
    jobs: [
      { type: "replacer", parameters: { deleteAllRules: true }, rules: [{ description: "disposable-agent",
        url: `^http://${gateway.replaceAll(".", "\\.")}:9191/api/v2/.*$`, matchType: "req_header",
        matchString: "Authorization", matchRegex: false, replacementString: `Bearer ${key}`, tokenProcessing: false }] },
      { type: "passiveScan-config", parameters: { scanOnlyInScope: true } },
      { type: "requestor", requests, tests: documentTests(documentUrl, true) },
      { type: "activeScan-policy", parameters: { name: "notegate-pilot" }, policyDefinition: {
        defaultStrength: "Low", defaultThreshold: "Off",
        rules: [6, 40012, 40018].map((id) => ({ id, strength: "Low", threshold: "Medium" })) } },
      { type: "activeScan", parameters: { context: "notegate", policy: "notegate-pilot", maxRuleDurationInMins: 1,
        maxScanDurationInMins: 5, threadPerHost: 1, delayInMs: 500, addQueryParam: false }, tests: [{ name: "Active attack requests generated",
        type: "stats", statistic: "stats.ascan.urls", operator: ">", value: 0, onFail: "error" }] },
      // Supported read parameters give this post-scan check a new Sites Tree node.
      { type: "requestor", requests: [{ name: "Post-scan authenticated read", method: "GET", responseCode: 200,
        url: `${documentUrl}?max_lines=5000&max_bytes=1048576` }],
        tests: documentTests(`${documentUrl}?max_lines=5000&max_bytes=1048576`, false) },
      { type: "passiveScan-wait", parameters: { maxDuration: 1 }, alwaysRun: true },
      { type: "report", parameters: { template: "traditional-json", reportDir: "/zap/wrk/reports", reportFile: "zap-report" }, alwaysRun: true }
    ]
  };
  evidence.seededRequests = requests.map(({ method, url }) => ({ method, url }));
  mkdirSync(join(work, "reports"));
  // Only the container's report directory is writable; its credential plan is read-only.
  chmodSync(work, 0o755);
  chmodSync(join(work, "reports"), 0o777);
  writeFileSync(join(work, "plan.yaml"), JSON.stringify(plan, null, 2), { mode: 0o644 });
  const scan = spawnSync("docker", ["run", "--rm", "--name", "notegate-zap-pilot", "--network", "notegate-dast",
    "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true", "--memory", "2g", "--cpus", "2",
    "-v", `${join(work, "plan.yaml")}:/zap/wrk/plan.yaml:ro`, "-v", `${join(work, "reports")}:/zap/wrk/reports:rw`, image,
    "zap.sh", "-cmd", "-silent", "-config", "autoupdate.checkOnStart=false", "-config", "stats.inmemory=true",
    "-autorun", "/zap/wrk/plan.yaml"], { encoding: "utf8", timeout: 600_000, maxBuffer: 16 * 1024 * 1024 });
  writeFileSync(join(results, "zap-execution.log"), redact(`${scan.stdout ?? ""}\n${scan.stderr ?? ""}`));
  const rawReport = join(work, "reports", "zap-report.json");
  const report = JSON.parse(readFileSync(rawReport, "utf8"));
  evidence.alerts = (report.site ?? []).flatMap((site) => (site.alerts ?? []).map((alert) => ({
    id: alert.pluginid, name: alert.name, risk: alert.riskdesc, confidence: alert.confidence, cwe: alert.cweid,
    description: alert.desc, solution: alert.solution,
    instances: (alert.instances ?? []).map(({ uri, method, param, attack, evidence: detail }) => ({ uri, method, param, attack, evidence: detail }))
  })));
  assert.equal(scan.status, 0, "ZAP execution/authentication/coverage failed; inspect sanitized zap-execution.log");
  evidence.authenticatedFixture = true; // ZAP itself checked the document ID/hash, not a separate HTTP client.
  evidence.activeRequestsGenerated = true; // The AF stats test above is a required execution check.
  evidence.completed = true;
} catch (error) {
  evidence.error = redact(error.message);
  process.exitCode = 1;
} finally {
  spawnSync("docker", ["rm", "-f", "notegate-zap-pilot"], { timeout: 10_000, stdio: "ignore" });
  spawnSync("docker", ["network", "rm", "notegate-dast"], { timeout: 10_000, stdio: "ignore" });
  const serialized = redact(JSON.stringify(evidence, null, 2));
  writeFileSync(join(results, "zap.json"), serialized);
  const escape = (text) => text.replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]);
  writeFileSync(join(results, "zap.html"), `<!doctype html><meta charset="utf-8"><title>NoteGate ZAP pilot</title><h1>Sanitized ZAP evidence</h1><pre>${escape(serialized)}</pre>`);
  writeFileSync(join(results, "summary-zap.md"), `## ZAP active scan pilot\n\n- Execution: ${evidence.completed ? "completed" : "failed"}\n- Authenticated fixture: ${evidence.authenticatedFixture}\n- Active requests generated: ${evidence.activeRequestsGenerated}\n- Alerts requiring triage: ${evidence.alerts.length}\n\nRules requested: path traversal, reflected XSS, SQL injection. One active thread, 500ms delay, five-minute scan limit. Alerts are reported; setup/authentication/coverage failures fail this pilot. This is not a full penetration test.\n`);
  rmSync(work, { recursive: true, force: true });
}
