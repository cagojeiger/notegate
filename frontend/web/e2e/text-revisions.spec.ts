import { expect, test, type Page } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { expectNoAccessibilityViolations } from "./support/accessibility";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const me: Me = { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
const space: Space = {
  id: "space-1", name: "Daily", sort_order: 0, navigation_pinned: true, user_mcp_enabled: true,
  default_external_access_enabled: true, default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true }, permission: "write", root_node_id: "root-1",
  created_at: "2026-10-01T00:00:00Z", updated_at: "2026-10-01T00:00:00Z"
};
const initialNode: RestNode = {
  id: "note-1", space_id: space.id, parent_id: space.root_node_id, name: "network.md", kind: "text", path: "/network.md",
  sort_order: 0, metadata: {}, external_access_enabled: true, write_locked: false, write_lock_sources: [],
  has_children: false, effective_write_locked: false, byte_len: 28, line_count: 4, content_sha256: "b".repeat(64),
  text_storage_format: "plain", text_at_rest_encryption: "none", created_by: me.account, updated_by: me.account,
  created_at: "2026-10-04T05:00:00Z", updated_at: "2026-10-04T05:20:00Z"
};
const oldContent = "# Network\nMTU: 1500\nCheck\n[Related note](/related.md)";
const newContent = "# Network\nMTU: 1450\nReady\n[Related note](/related.md)";
const revision = {
  id: "revision-1", node_id: initialNode.id, content_sha256: "a".repeat(64), byte_len: oldContent.length, line_count: 4,
  written_at: "2026-10-04T05:12:00Z", author_id: me.account.id, group_id: "group-1", source: "mcp", purpose: "Document the original MTU before correcting the network configuration.", superseded_at: "2026-10-04T05:20:00Z"
};
const textPath = `/api/v1/spaces/${space.id}/text/${initialNode.id}`;
const pageInfo = (returned: number) => ({ limit: 50, returned, has_more: false, next_cursor: null });

async function setup(page: Page, options: { mobile?: boolean; readOnly?: boolean; purpose?: string; previousContent?: string; currentContent?: string } = {}) {
  await page.emulateMedia({ colorScheme: options.mobile ? "light" : "dark" });
  await page.setViewportSize(options.mobile ? { width: 390, height: 844 } : { width: 1440, height: 1000 });
  const historicalContent = options.previousContent ?? oldContent;
  const historicalRevision = { ...revision, byte_len: historicalContent.length, line_count: historicalContent.split("\n").length, purpose: options.purpose ?? revision.purpose };
  let content = options.currentContent ?? newContent;
  let node = { ...initialNode, byte_len: content.length, line_count: content.split("\n").length, content_sha256: content === historicalContent ? revision.content_sha256 : initialNode.content_sha256 };
  const requests: { path: string; method: string; body: Record<string, unknown> | null }[] = [];
  await routeJsonApi(page, (url, request) => {
    requests.push({ path: url.pathname, method: request.method(), body: request.postDataJSON() });
    if (url.pathname === "/api/v1/me") return me;
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/spaces") return { spaces: [{ ...space, permission: options.readOnly ? "read" : "write" }], page: pageInfo(1) };
    if (url.pathname.endsWith(`/nodes/${space.root_node_id}/children`)) return { parent: { id: space.root_node_id, path: "/" }, children: [node], page: pageInfo(1) };
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) return { nodes: [node], page: pageInfo(1) };
    if (url.pathname.endsWith(`/nodes/${node.id}`)) return node;
    if (url.pathname.endsWith(`/nodes/${node.id}/reveal`)) return { ancestors: [], target: node };
    if (url.pathname === `${textPath}/revisions`) return { current: { content_sha256: node.content_sha256, purpose: node.content_sha256 === initialNode.content_sha256 ? "Correct MTU to 1450 after verifying the overlay network." : null }, revisions: [historicalRevision], page: pageInfo(1) };
    if (url.pathname === `${textPath}/revisions/${revision.id}`) return { revision: historicalRevision, content: historicalContent };
    if (url.pathname === `${textPath}/revisions/${revision.id}/restore`) {
      content = historicalContent;
      node = { ...node, content_sha256: revision.content_sha256, updated_at: "2026-10-04T05:30:00Z" };
      return { node_id: node.id, content_sha256: node.content_sha256, byte_len: content.length, line_count: content.split("\n").length };
    }
    if (url.pathname === textPath) return {
      node: { id: node.id, path: node.path }, text: { node_id: node.id, storage_format: "plain", content,
        content_sha256: node.content_sha256, byte_len: content.length, line_count: content.split("\n").length, start_line: 1, end_line: content.split("\n").length,
        returned_lines: content.split("\n").length, truncated: false, next_start_line: null, updated_by: me.account, updated_at: node.updated_at }
    };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled request: ${request.method()} ${url.pathname}`);
  });
  await page.goto("/");
  if (options.mobile) await page.getByRole("button", { name: "Toggle left sidebar" }).click();
  await page.getByRole("button", { name: node.name }).first().click();
  await expect(page.getByRole("heading", { name: "Network", exact: true })).toBeVisible();
  return requests;
}

test("lazily opens revision comparison and restores with the reviewed current hash", async ({ page }) => {
  const requests = await setup(page);
  expect(requests.filter((r) => r.path.includes("/revisions"))).toHaveLength(0);
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Version history · network.md" });
  await expect(dialog.getByLabel("Version comparison", { exact: true })).toBeVisible();
  await expect(dialog.getByText("MTU: 1500", { exact: true })).toBeVisible();
  await expect(dialog.getByText("MTU: 1450", { exact: true })).toBeVisible();
  await expect(dialog.getByLabel("Change reason")).toContainText(revision.purpose);
  await dialog.getByRole("button", { name: "Current saved version", exact: true }).click();
  await expect(dialog.getByRole("tab", { name: "Full version" })).toHaveAttribute("aria-selected", "true");
  await expect(dialog.getByRole("tab", { name: "Compare changes" })).toBeDisabled();
  await expect(dialog.getByLabel("Change reason")).toContainText("Correct MTU to 1450");
  await dialog.getByRole("button", { name: /Edited via MCP/ }).click();
  await dialog.getByRole("tab", { name: "Full version" }).click();
  await expect(dialog.getByRole("heading", { name: "Network", exact: true })).toBeVisible();
  await page.screenshot({ path: "test-results/text-revisions-full-version.png" });
  await dialog.getByRole("tab", { name: "Compare changes" }).click();
  await expect(dialog.getByLabel("Version comparison", { exact: true })).toBeVisible();
  await page.screenshot({ path: "test-results/text-revisions-desktop.png" });
  await dialog.getByRole("button", { name: "Restore this version" }).click();
  expect(requests.filter((r) => r.method === "POST")).toHaveLength(0);
  await page.screenshot({ path: "test-results/text-revisions-confirm-restore.png" });
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeEnabled();
  expect(requests.filter((r) => r.method === "POST")).toHaveLength(0);
  await dialog.getByRole("button", { name: "Restore this version" }).click();
  await dialog.getByRole("button", { name: "Confirm restore" }).click();
  await expect(dialog).not.toBeVisible();
  expect(requests.find((r) => r.path.endsWith("/restore"))?.body).toEqual({ expected_sha256: initialNode.content_sha256 });
  await expect(page.getByText(/MTU: 1500/).first()).toBeVisible();
});

test("editing after restore uses the restored body while background reads are delayed", async ({ page }) => {
  await setup(page);
  let restoring = false;
  let pendingReads = 0;
  let releaseRead!: () => void;
  const readBarrier = new Promise<void>((resolve) => { releaseRead = resolve; });
  await page.route(`**${textPath}/revisions/${revision.id}/restore`, async (route) => {
    restoring = true;
    await route.fallback();
  });
  await page.route(`**${textPath}?*`, async (route) => {
    if (restoring) {
      pendingReads++;
      await readBarrier;
    }
    await route.fallback();
  });
  try {
    await page.getByRole("button", { name: "Version history", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog.getByLabel("Version comparison", { exact: true })).toBeVisible();
    await dialog.getByRole("button", { name: "Restore this version" }).click();
    await dialog.getByRole("button", { name: "Confirm restore" }).click();
    await expect(dialog).not.toBeVisible();
    await expect.poll(() => pendingReads).toBeGreaterThan(0);
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    const editor = page.getByRole("textbox", { name: "Edit text content" });
    await expect(editor).toHaveValue(oldContent);
    await editor.fill("New draft after restoring");
    const refreshed = page.waitForResponse((response) => new URL(response.url()).pathname === textPath && response.request().method() === "GET");
    releaseRead();
    await refreshed;
    await expect(editor).toHaveValue("New draft after restoring");
    await expect(page.getByRole("button", { name: "Save", exact: true })).toBeEnabled();
  } finally {
    releaseRead();
  }
});

test("history browsing preserves unsaved edits and blocks restore", async ({ page }) => {
  await setup(page);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("textbox", { name: "Edit text content" }).fill("Unsaved draft stays here");
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText(/Unsaved edits are preserved/)).toBeVisible();
  await dialog.getByRole("tab", { name: "Full version" }).click();
  const originalUrl = page.url();
  await dialog.getByRole("link", { name: "Related note" }).click();
  expect(page.url()).toBe(originalUrl);
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole("textbox", { name: "Edit text content" })).toHaveValue("Unsaved draft stays here");
});

test("mobile opens history from More actions and keeps read-only history accessible", async ({ page }) => {
  await setup(page, { mobile: true, readOnly: true });
  await page.getByRole("button", { name: "More actions", exact: true }).first().click();
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByLabel("Version comparison", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeDisabled();
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await expectNoAccessibilityViolations(page);
  const comparison = dialog.getByRole("region", { name: "Version comparison", exact: true });
  await comparison.focus();
  await page.keyboard.press("End");
  await expect.poll(() => comparison.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await page.keyboard.press("Home");
  await expect.poll(() => comparison.evaluate((el) => el.scrollTop)).toBe(0);
  await page.screenshot({ path: "test-results/text-revisions-mobile.png" });
});

test("a stale restore asks for review instead of retrying or overwriting", async ({ page }) => {
  const requests = await setup(page);
  let restoreCalls = 0;
  await page.route(`**${textPath}/revisions/${revision.id}/restore`, async (route) => {
    restoreCalls++;
    await route.fulfill({ status: 409, contentType: "application/json", body: JSON.stringify({ kind: "conflict", message: "stale" }) });
  });
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeEnabled();
  await dialog.getByRole("button", { name: "Restore this version" }).click();
  await dialog.getByRole("button", { name: "Confirm restore" }).click();
  await expect(dialog.getByText(/The current document changed/)).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Confirm restore" })).toBeDisabled();
  expect(restoreCalls).toBe(1);
  expect(requests.filter((r) => r.method === "PUT")).toHaveLength(0);
});


test("keyboard navigation stays in history and returns to the header trigger", async ({ page }) => {
  await setup(page);
  const trigger = page.getByRole("button", { name: "Version history", exact: true });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByLabel("Version comparison", { exact: true })).toBeVisible();
  const closeIcon = dialog.getByRole("button", { name: "Close", exact: true }).first();
  const restore = dialog.getByRole("button", { name: "Restore this version" });
  await expect(closeIcon).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(restore).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(closeIcon).toBeFocused();
  const compare = dialog.getByRole("tab", { name: "Compare changes" });
  await compare.focus();
  await page.keyboard.press("ArrowRight");
  await expect(dialog.getByRole("tab", { name: "Full version" })).toBeFocused();
  await expect(dialog.getByRole("heading", { name: "Network", exact: true })).toBeVisible();
  await page.keyboard.press("ArrowLeft");
  await expect(compare).toBeFocused();
  await expectNoAccessibilityViolations(page);
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(trigger).toBeFocused();
});

test("small phones keep long reasons and restore controls usable and return focus to More", async ({ page }) => {
  const purpose = "검증된 네트워크 설정과 원인을 문서에 반영하고 변경 내용을 다시 확인합니다. ".repeat(5).slice(0, 200);
  const requests = await setup(page, { mobile: true, purpose });
  await page.setViewportSize({ width: 320, height: 568 });
  const more = page.getByRole("button", { name: "More actions", exact: true }).first();
  await more.focus();
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: "Version history", exact: true }).focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog");
  const comparison = dialog.getByLabel("Version comparison", { exact: true });
  await expect(comparison).toBeVisible();
  await expect(dialog.getByLabel("Change reason")).toContainText(purpose);
  expect((await comparison.boundingBox())?.height).toBeGreaterThanOrEqual(100);
  await comparison.scrollIntoViewIfNeeded();
  await expect(comparison.getByText("MTU: 1500", { exact: true })).toBeInViewport();
  await page.screenshot({ path: "test-results/text-revisions-small-phone-comparison.png" });
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await dialog.getByRole("button", { name: "Restore this version" }).click();
  await expect(dialog.getByRole("button", { name: "Confirm restore" })).toBeInViewport();
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.screenshot({ path: "test-results/text-revisions-small-phone.png" });
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(requests.filter((r) => r.method === "POST")).toHaveLength(0);
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(more).toBeFocused();
});

test("a conflicted restore reloads the new baseline and requires another confirmation", async ({ page }) => {
  const requests = await setup(page);
  const changedHash = "c".repeat(64);
  let restoreCalls = 0;
  await page.route(`**${textPath}/revisions/${revision.id}/restore`, async (route) => {
    restoreCalls++;
    if (restoreCalls === 1) {
      await route.fulfill({ status: 409, contentType: "application/json", body: JSON.stringify({ kind: "conflict", message: "stale" }) });
    } else {
      expect(route.request().postDataJSON()).toEqual({ expected_sha256: changedHash });
      await route.fallback();
    }
  });
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeEnabled();
  await dialog.getByRole("button", { name: "Restore this version" }).click();
  await dialog.getByRole("button", { name: "Confirm restore" }).click();
  await expect(dialog.getByRole("alert")).toContainText("The current document changed");
  await dialog.getByRole("button", { name: "Current saved version", exact: true }).click();
  await dialog.getByRole("button", { name: /Edited via MCP/ }).click();
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeDisabled();
  expect(restoreCalls).toBe(1);
  await page.route(`**${textPath}?*`, async (route) => {
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      node: { id: initialNode.id, path: initialNode.path }, text: { node_id: initialNode.id, storage_format: "plain", content: newContent.replace("1450", "1400"),
        content_sha256: changedHash, byte_len: newContent.length, line_count: 4, start_line: 1, end_line: 4,
        returned_lines: 4, truncated: false, next_start_line: null, updated_by: me.account, updated_at: "2026-10-04T05:25:00Z" }
    }) });
  });
  await dialog.getByRole("button", { name: "Reload saved version" }).click();
  await expect(dialog.getByText("MTU: 1400", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("alert")).not.toBeVisible();
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeEnabled();
  expect(restoreCalls).toBe(1);
  await dialog.getByRole("button", { name: "Restore this version" }).click();
  expect(restoreCalls).toBe(1);
  await dialog.getByRole("button", { name: "Confirm restore" }).click();
  await expect(dialog).not.toBeVisible();
  expect(restoreCalls).toBe(2);
  expect(requests.filter((r) => r.method === "PUT")).toHaveLength(0);
});

for (const mobile of [false, true]) {
  for (const colorScheme of ["dark", "light"] as const) {
    test(`long comparisons reveal changes and navigate locally on ${mobile ? "mobile" : "desktop"} in ${colorScheme}`, async ({ page }) => {
      const before = Array.from({ length: 320 }, (_, index) => index === 0 ? "# Network" : `Line ${index + 1}`);
      const after = before.map((line, index) => (index >= 80 && index < 105) || index === 230 ? `Updated ${line}` : line);
      const requests = await setup(page, { mobile, previousContent: before.join("\n"), currentContent: after.join("\n") });
      await page.emulateMedia({ colorScheme });
      const preview = page.getByRole("region", { name: "Document preview", exact: true });
      await preview.focus();
      await expect(preview).toBeFocused();
      await page.keyboard.press("End");
      await expect.poll(() => preview.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
      await page.keyboard.press("Home");
      await expect.poll(() => preview.evaluate((element) => element.scrollTop)).toBe(0);
      if (mobile) await page.getByRole("button", { name: "More actions", exact: true }).first().click();
      await page.getByRole("button", { name: "Version history", exact: true }).click();
      const dialog = page.getByRole("dialog");
      const comparison = dialog.getByRole("region", { name: "Version comparison", exact: true });
      await expect(comparison.getByText("Updated Line 81", { exact: true })).toBeInViewport();
      await expect(comparison.getByText("Line 2", { exact: true })).toHaveCount(0);
      await expect(dialog.getByLabel("Change summary")).toHaveText("+26 added−26 removed");
      await expect(dialog.getByText("Change 1 of 2", { exact: true })).toBeVisible();
      await expect(dialog.getByRole("button", { name: "Previous change" })).toBeDisabled();
      await expect(dialog.getByText(/Cumulative comparison:/)).toBeVisible();
      await expect(dialog.getByLabel("Change reason")).toContainText("Selected version save reason:");
      await expect(dialog.getByText(/Restores the entire selected version/)).toBeVisible();
      const bodyReads = () => requests.filter((request) => request.method === "GET" && (request.path === textPath || request.path === `${textPath}/revisions/${revision.id}`)).length;
      const reads = bodyReads();
      const dialogBox = await dialog.boundingBox();
      const outsideScroll = await dialog.getByRole("region", { name: "Version content" }).evaluate((element) => element.scrollTop);
      const next = dialog.getByRole("button", { name: "Next change" });
      await next.focus();
      await page.keyboard.press("Enter");
      await expect(comparison.getByText("Updated Line 231", { exact: true })).toBeInViewport();
      await expect(dialog.getByText("Change 2 of 2", { exact: true })).toBeVisible();
      await expect(next).toBeDisabled();
      expect(await dialog.boundingBox()).toEqual(dialogBox);
      expect(await dialog.getByRole("region", { name: "Version content" }).evaluate((element) => element.scrollTop)).toBe(outsideScroll);
      await dialog.getByRole("button", { name: "Previous change" }).click();
      await expect(comparison.getByText("Updated Line 81", { exact: true })).toBeInViewport();
      await expectNoAccessibilityViolations(page);
      await page.screenshot({ path: `test-results/text-revisions-changes-${mobile ? "mobile" : "desktop"}-${colorScheme}.png` });
      await comparison.getByRole("button", { name: "77 unchanged lines · Expand", exact: true }).click();
      await expect(comparison.getByText("Line 2", { exact: true }).first()).toBeVisible();
      await comparison.getByRole("button", { name: "77 unchanged lines · Collapse", exact: true }).click();
      await expect(comparison.getByText("Line 2", { exact: true })).toHaveCount(0);
      expect(bodyReads()).toBe(reads);
    });
  }
}

test("identical saved content states that there are no changes and cannot be restored", async ({ page }) => {
  await setup(page, { currentContent: oldContent });
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("No changes compared with the current saved version.", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Next change" })).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "Restore this version" })).toBeDisabled();
  await dialog.getByRole("tab", { name: "Full version" }).click();
  await expect(dialog.getByRole("heading", { name: "Network", exact: true })).toBeVisible();
});

test("switching versions resets expanded context and distinguishes saves within the same minute", async ({ page }) => {
  const before = Array.from({ length: 180 }, (_, index) => index === 0 ? "# Network" : `Line ${index + 1}`);
  const after = before.map((line, index) => index === 80 || index === 150 ? `Updated ${line}` : line);
  await setup(page, { previousContent: before.join("\n"), currentContent: after.join("\n") });
  const older = { ...revision, id: "revision-2", written_at: "2026-10-04T05:12:01Z", purpose: "Record the earlier experiment." };
  await page.route(`**${textPath}/revisions?*`, (route) => route.fulfill({ json: { revisions: [revision, older], page: pageInfo(2) } }));
  await page.route(`**${textPath}/revisions/${older.id}`, (route) => route.fulfill({ json: { revision: older, content: before.join("\n") } }));
  await page.getByRole("button", { name: "Version history", exact: true }).click();
  const dialog = page.getByRole("dialog");
  const comparison = dialog.getByRole("region", { name: "Version comparison", exact: true });
  await comparison.getByRole("button", { name: "77 unchanged lines · Expand", exact: true }).click();
  await dialog.getByRole("button", { name: "Next change" }).click();
  await expect(dialog.getByText("Change 2 of 2", { exact: true })).toBeVisible();
  const versions = dialog.getByRole("navigation", { name: "Saved versions" });
  const first = versions.getByRole("button", { name: /Document the original MTU/ });
  const second = versions.getByRole("button", { name: /Record the earlier experiment/ });
  await expect(first.locator("time")).toHaveText(/:00/);
  await expect(second.locator("time")).toHaveText(/:01/);
  await second.click();
  await expect(dialog.getByText("Change 1 of 2", { exact: true })).toBeVisible();
  await expect(comparison.getByText("Updated Line 81", { exact: true })).toBeInViewport();
  await expect(comparison.getByText("Line 2", { exact: true })).toHaveCount(0);
  await expect(dialog.getByLabel("Change reason")).toContainText(older.purpose);
});
