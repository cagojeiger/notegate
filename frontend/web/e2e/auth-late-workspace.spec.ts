import { expect, test } from "@playwright/test";

import { makeRestNode, makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { openSettings } from "./support/settings";
import { usageResponse } from "./support/usage";

test("a previous owner's late space creation preserves the new owner's unsaved document", async ({ page }) => {
  let owner = "a";
  let createCalls = 0;
  let finishCreate!: () => void;
  const a = makeSpace({ id: "space-a", name: "Space A", root_node_id: "root-a" });
  const b = makeSpace({ id: "space-b", name: "Space B", root_node_id: "root-b" });
  const created = makeSpace({ id: "late-created-a", name: "Late created A", root_node_id: "root-new-a" });
  const content = "# Owner B\nSaved B body";
  const note = makeRestNode({ id: "note-b", space_id: b.id, parent_id: b.root_node_id,
    name: "b.md", path: "/b.md", content_sha256: "b-sha", byte_len: content.length, line_count: 2 });
  const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };

  await page.clock.install();
  await page.addInitScript(() => {
    const channel = new MessageChannel();
    const state = window as typeof window & { lateCreateConsumed?: boolean };
    channel.port1.onmessage = () => { state.lateCreateConsumed = true; };
    const originalJson = Response.prototype.json;
    Response.prototype.json = async function () {
      const body = await originalJson.call(this);
      if (new URL(this.url, location.href).pathname === "/api/v1/spaces" && body.id === "late-created-a") {
        // Wait for the consumer's mutation callbacks before checking the draft.
        channel.port2.postMessage(null);
      }
      return body;
    };
  });
  await routeJsonApi(page, (url) => {
    const space = owner === "a" ? a : b;
    if (url.pathname === "/api/v1/me") return {
      account: { id: `user-${owner}`, kind: "user", display_name: `User ${owner}` },
      user: { email: `${owner}@example.invalid` },
      capabilities: { can_create_space: true, can_manage_agents: true }
    };
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname.endsWith("/children")) return {
      parent: { id: space.root_node_id, path: "/" }, children: owner === "b" ? [note] : [], page: meta
    };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/nodes/note-b/reveal")) return { ancestors: [], target: note };
    if (url.pathname.endsWith("/nodes/note-b")) return note;
    if (url.pathname.endsWith("/text/note-b")) return {
      node: { id: note.id, path: note.path }, text: { node_id: note.id, storage_format: "plain", content,
        content_sha256: note.content_sha256, byte_len: content.length, line_count: 2, start_line: 1, end_line: 2,
        returned_lines: 2, truncated: false, next_start_line: null, updated_by: note.updated_by, updated_at: note.updated_at }
    };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled API request: ${url.pathname}`);
  });
  await page.route("**/api/v1/spaces", async (route) => {
    if (route.request().method() !== "POST") return route.fallback();
    createCalls += 1;
    await new Promise<void>((resolve) => { finishCreate = resolve; });
    await route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify(created) });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Add space", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New space", exact: true });
  await dialog.getByRole("textbox", { name: "Space name", exact: true }).fill(created.name);
  await dialog.getByRole("button", { name: "Create", exact: true }).click();
  await expect.poll(() => createCalls).toBe(1);

  owner = "b";
  await page.clock.runFor(6_000);
  const me = page.waitForResponse((response) => new URL(response.url()).pathname === "/api/v1/me");
  await page.evaluate(() => {
    window.dispatchEvent(new Event("offline"));
    window.dispatchEvent(new Event("online"));
  });
  await me;
  await expect(page.getByRole("button", { name: b.name, exact: true })).toBeVisible();
  await expect(dialog).toHaveCount(0);
  await page.getByRole("button", { name: note.name, exact: true }).click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.getByRole("textbox", { name: "Edit text content", exact: true });
  const draft = "Keep User B unsaved draft";
  await editor.fill(draft);
  await expect(editor).toHaveValue(draft);

  finishCreate();
  await expect.poll(() => page.evaluate(() => (window as typeof window & { lateCreateConsumed?: boolean }).lateCreateConsumed)).toBe(true);
  await expect(editor).toHaveValue(draft);
  await expect(page.getByRole("button", { name: "Save", exact: true })).toBeEnabled();
});

test("a previous owner's late logout failure preserves the new owner's unsaved document", async ({ page }, testInfo) => {
  let owner = "a";
  let logoutStarted = false;
  let finishLogout!: () => void;
  const a = makeSpace({ id: "space-a", name: "Space A", root_node_id: "root-a" });
  const b = makeSpace({ id: "space-b", name: "Space B", root_node_id: "root-b" });
  const content = "# Owner B\nSaved B body";
  const note = makeRestNode({ id: "note-b", space_id: b.id, parent_id: b.root_node_id,
    name: "b.md", path: "/b.md", content_sha256: "b-sha", byte_len: content.length, line_count: 2 });
  const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
  const dates = { created_at: "2026-10-22T00:00:00Z", expires_at: "2026-11-22T00:00:00Z", idle_expires_at: "2026-11-10T00:00:00Z" };
  await page.clock.install();
  await page.addInitScript(() => {
    const channel = new MessageChannel();
    const state = window as typeof window & { oldLogoutConsumed?: boolean };
    channel.port1.onmessage = () => { state.oldLogoutConsumed = true; };
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (...args) => {
      const response = await originalFetch(...args);
      if (new URL(response.url, location.href).pathname === "/auth/logout") channel.port2.postMessage(null);
      return response;
    };
  });
  await routeJsonApi(page, (url) => {
    const space = owner === "a" ? a : b;
    if (url.pathname === "/api/v1/me") return {
      account: { id: `user-${owner}`, kind: "user", display_name: `User ${owner}` },
      user: { email: `${owner}@example.invalid` },
      capabilities: { can_create_space: true, can_manage_agents: true }
    };
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/me/sessions") return { sessions: owner === "a" ? [{ ...dates, id: "session-a", current: true, client_environment: "Browser A", last_used_at: null }] : [] };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: owner === "b" ? [note] : [], page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/nodes/note-b/reveal")) return { ancestors: [], target: note };
    if (url.pathname.endsWith("/nodes/note-b")) return note;
    if (url.pathname.endsWith("/text/note-b")) return {
      node: { id: note.id, path: note.path }, text: { node_id: note.id, storage_format: "plain", content,
        content_sha256: note.content_sha256, byte_len: content.length, line_count: 2, start_line: 1, end_line: 2,
        returned_lines: 2, truncated: false, next_start_line: null, updated_by: note.updated_by, updated_at: note.updated_at }
    };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled API request: ${url.pathname}`);
  });
  await page.route("**/api/v1/me/sessions/session-a", (route) => route.fulfill({ status: 204 }));
  await page.route("**/auth/logout", async (route) => {
    logoutStarted = true;
    await new Promise<void>((resolve) => { finishLogout = resolve; });
    // A failed endpoint response does not clear the newer browser cookie.
    await route.fulfill({ status: 503 });
  });
  await page.goto("/");
  const settings = await openSettings(page, "Account");
  await settings.getByRole("button", { name: "Sign out Browser A (this session)", exact: true }).click();
  await settings.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect.poll(() => logoutStarted).toBe(true);
  owner = "b";
  await page.clock.runFor(6_000);
  const me = page.waitForResponse((response) => new URL(response.url()).pathname === "/api/v1/me");
  await page.evaluate(() => { window.dispatchEvent(new Event("offline")); window.dispatchEvent(new Event("online")); });
  await me;
  await expect(page.getByRole("button", { name: b.name, exact: true })).toBeVisible();
  await page.getByRole("button", { name: note.name, exact: true }).click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.getByRole("textbox", { name: "Edit text content", exact: true });
  const draft = "Keep User B unsaved draft";
  await editor.fill(draft);
  finishLogout();
  await expect.poll(() => page.evaluate(() => (window as typeof window & { oldLogoutConsumed?: boolean }).oldLogoutConsumed)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("late-logout-b.png") });
  await expect(editor).toHaveValue(draft);
});
