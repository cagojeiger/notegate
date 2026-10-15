import { expect, test, type Page } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Performance fixture",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_external_access_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-24T00:00:00Z",
  updated_at: "2026-07-24T00:00:00Z"
};

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

for (const viewport of [
  { name: "desktop", width: 1440, height: 900, opensOverlay: false },
  { name: "tablet", width: 900, height: 1024, opensOverlay: false },
  { name: "mobile", width: 390, height: 844, opensOverlay: true }
]) {
  test(`backs off idle active-space refreshes on ${viewport.name}`, async ({ page }) => {
    const requests = new Map<string, number>();
    const syncPath = `/api/v1/spaces/${space.id}/file-change-sync`;
    await page.clock.install();
    const waitForSync = await observeCompletedPolling(page, syncPath);
    await page.addInitScript(() => {
      Math.random = () => 0.5;
    });
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await routeJsonApi(page, (url) => {
      requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
      return responseFor(url);
    });

    await page.goto("/");
    if (viewport.opensOverlay) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }

    const tree = page.getByRole("tree", { name: "Files" });
    await expect(tree).toBeVisible();
    await expect(tree.getByRole("button", { name: "note.md", exact: true })).toBeVisible();
    await waitForSync(1);
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);
    await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1_000));

    for (const [elapsedMs, expectedRequests] of [
      [30_001, 2],
      [60_001, 3],
      [120_001, 4],
      [300_001, 5],
      [300_001, 6]
    ] as const) {
      await page.clock.runFor(elapsedMs);
      await waitForSync(expectedRequests);
      expect(count(requests, syncPath)).toBe(expectedRequests);
    }

    expect(count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    expect(count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);

    if (viewport.name === "desktop") {
      await page.context().setOffline(true);
      await page.context().setOffline(false);
      expect(count(requests, `/api/v1/spaces/${space.id}/file-change-sync`)).toBe(6);

      await page.clock.runFor(30_001);
      await waitForSync(7);
      expect(count(requests, syncPath)).toBe(7);
    }
  });
}

test("schedules the next idle interval after a delayed sync finishes", async ({ page }) => {
  const syncPath = `/api/v1/spaces/${space.id}/file-change-sync`;
  let requests = 0;
  let releaseResponse!: () => void;
  const delayedResponse = new Promise<void>((resolve) => { releaseResponse = resolve; });
  await page.clock.install();
  const waitForSync = await observeCompletedPolling(page, syncPath);
  await page.addInitScript(() => { Math.random = () => 0.5; });
  await routeJsonApi(page, responseFor);
  await page.route((url) => url.pathname === syncPath, async (route) => {
    requests += 1;
    if (requests === 2) await delayedResponse;
    await route.fulfill({ json: responseFor(new URL(route.request().url())) });
  });

  try {
    await page.goto("/");
    await expect(page.getByRole("tree", { name: "Files" })).toBeVisible();
    await waitForSync(1);
    await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1_000));

    await page.clock.runFor(30_001);
    await expect.poll(() => requests).toBe(2);
    await page.clock.runFor(120_001);
    expect(await page.evaluate(() => window.__startedPollingRequests)).toBe(2);
    await waitForSync(1);

    releaseResponse();
    await waitForSync(2);
    await page.clock.runFor(59_999);
    expect(await page.evaluate(() => window.__startedPollingRequests)).toBe(2);
    await page.clock.runFor(2);
    await waitForSync(3);
    expect(requests).toBe(3);
  } finally {
    releaseResponse();
  }
});

test("opens a recent node with one reveal request and no canonical node request", async ({ page }) => {
  const requests = new Map<string, number>();
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeJsonApi(page, (url) => {
    requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
    return responseFor(url);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: "note.md" }).click();

  const editor = page.locator('[data-editor-group][data-active="true"]');
  await expect(editor).toContainText("note.md");
  await expect(editor).toContainText("Request budget");
  await expect.poll(() => count(
    requests,
    `/api/v1/spaces/${space.id}/nodes/text-1/reveal`
  )).toBe(1);
  await expect.poll(() => count(
    requests,
    `/api/v1/spaces/${space.id}/text/text-1`
  )).toBe(1);
  expect(count(requests, `/api/v1/spaces/${space.id}/nodes/text-1`)).toBe(0);
});

test("loads deferred global dialogs after the production entry renders", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeJsonApi(page, (url) => responseFor(url));
  await page.goto("/");

  await page.getByRole("button", { name: "History" }).click();
  const history = page.getByRole("dialog", { name: "History" });
  await expect(history).toBeVisible();
  await history.getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "Add space" }).click();
  await expect(page.getByRole("dialog", { name: "New space" })).toBeVisible();
});

test("refreshes restored editors through one focus-owned Space sync", async ({ page }) => {
  const requests = new Map<string, number>();
  const nodes = [1, 2, 3].map((index) => restoredTextNode(index));
  const syncPath = `/api/v1/spaces/${space.id}/file-change-sync`;
  let externalRevision = 1;

  await page.clock.install();
  await page.addInitScript(({ restoredNodes, spaceId }) => {
    window.localStorage.setItem("notegate.lastActiveSpaceId", spaceId);
    window.localStorage.setItem(`notegate.workbench.v1.space.${spaceId}`, JSON.stringify({
      version: 1,
      spaceId,
      updatedAt: Date.now(),
      activeGroupIndex: 2,
      groups: restoredNodes.map((node) => ({ node, mode: "preview", back: [], forward: [] }))
    }));
  }, { restoredNodes: nodes, spaceId: space.id });
  await routeJsonApi(page, (url) => {
    requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
    if (url.pathname === syncPath) {
      if (count(requests, syncPath) === 3) {
        externalRevision = 2;
        return {
          changes: [{
            id: 11,
            node_id: "text-2",
            op_type: "text.write",
            item_kind: "text",
            affected_parent_ids: [space.root_node_id],
            parent_scope_known: true,
            path_changed: false,
            subtree_changed: false,
            write_lock_changed: false
          }],
          next_after_id: 11,
          has_more: false,
          resync_required: false
        };
      }
      return {
        changes: [],
        next_after_id: 10,
        has_more: false,
        resync_required: false
      };
    }
    return restoredEditorsResponseFor(url, externalRevision);
  });

  await page.goto("/");
  await expect(page.locator("[data-editor-group]")).toHaveCount(3);
  await expect.poll(() => restoredEditorRequestCount(requests)).toBe(6);
  await expect.poll(() => count(requests, syncPath)).toBe(1);
  await expect.poll(() => count(requests, "/api/v1/me")).toBe(1);
  await expect.poll(() => count(requests, "/api/v1/me/usage")).toBe(1);
  await expect.poll(() => count(requests, "/api/v1/spaces")).toBe(1);
  await expect.poll(() => count(
    requests,
    `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`
  )).toBe(1);
  await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);

  const idleBaseline = totalRequests(requests);
  await leaveAndReturnAfterStale(page);

  await expect.poll(() => count(requests, syncPath)).toBe(2);
  await expect.poll(() => totalRequests(requests) - idleBaseline).toBe(4);
  expect(restoredEditorRequestCount(requests)).toBe(6);

  const changedBaseline = totalRequests(requests);
  await leaveAndReturnAfterStale(page);

  await expect.poll(() => count(requests, syncPath)).toBe(3);
  await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes/text-2`)).toBe(2);
  await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/text/text-2`)).toBe(2);
  await expect(page.locator("[data-editor-group]").nth(1)).toContainText("Request budget 2 updated");
  expect(count(requests, `/api/v1/spaces/${space.id}/nodes/text-1`)).toBe(1);
  expect(count(requests, `/api/v1/spaces/${space.id}/text/text-1`)).toBe(1);
  expect(count(requests, `/api/v1/spaces/${space.id}/nodes/text-3`)).toBe(1);
  expect(count(requests, `/api/v1/spaces/${space.id}/text/text-3`)).toBe(1);
  expect(totalRequests(requests) - changedBaseline).toBe(8);
});

test("refreshes a folder lock once while updating its open descendant", async ({ page }) => {
  const basePath = `/api/v1/spaces/${space.id}`;
  const syncPath = `${basePath}/file-change-sync`;
  const folderPath = `${basePath}/nodes/folder-1`;
  const childPath = `${basePath}/nodes/text-1`;
  const textPath = `${basePath}/text/text-1`;
  const folder: RestNode = {
    ...textNode(), id: "folder-1", name: "parent", path: "/parent",
    kind: "folder", has_children: true, content_sha256: undefined
  };
  let locked = false;
  let pendingChange = false;
  let changeId = 10;
  let parentResponse = Promise.resolve();
  let releaseParent = () => {};
  const sources = () => locked ? [{ node_id: folder.id, name: folder.name, path: folder.path }] : [];
  const currentFolder = (): RestNode => ({
    ...folder, write_locked: locked, effective_write_locked: locked, write_lock_sources: sources()
  });
  const currentChild = (): RestNode => ({
    ...textNode(), parent_id: folder.id, path: "/parent/note.md",
    effective_write_locked: locked, write_lock_sources: sources()
  });

  await page.clock.install();
  const waitForSync = await observeCompletedPolling(page, syncPath);
  await page.addInitScript((paths) => {
    Math.random = () => 0.5;
    window.__lockFetchStarts = {};
    const originalFetch = window.fetch;
    window.fetch = function (input, init) {
      const path = new URL(input instanceof Request ? input.url : String(input), location.href).pathname;
      if (paths.includes(path)) {
        window.__lockFetchStarts[path] = (window.__lockFetchStarts[path] ?? 0) + 1;
      }
      return originalFetch.call(window, input, init);
    };
  }, [folderPath, childPath, textPath]);
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeJsonApi(page, (url) => {
    if (url.pathname === syncPath) {
      const changes = pendingChange ? [{
        id: ++changeId, node_id: folder.id, op_type: "item.update", item_kind: "folder",
        affected_parent_ids: [space.root_node_id], parent_scope_known: true,
        path_changed: false, subtree_changed: false, write_lock_changed: true
      }] : [];
      pendingChange = false;
      return { changes, next_after_id: changeId, has_more: false, resync_required: false };
    }
    if (url.pathname === `${basePath}/nodes/${space.root_node_id}/children`
      || url.pathname === `${folderPath}/children`) {
      const isRoot = url.pathname.includes(`/${space.root_node_id}/`);
      return {
        parent: { id: isRoot ? space.root_node_id : folder.id, path: isRoot ? "/" : folder.path },
        children: [isRoot ? currentFolder() : currentChild()],
        page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
      };
    }
    if (url.pathname === `${basePath}/nodes`) {
      return {
        nodes: [currentChild()],
        page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
      };
    }
    if (url.pathname === `${childPath}/reveal`) {
      return { ancestors: [currentFolder()], target: currentChild() };
    }
    if (url.pathname === childPath) return currentChild();
    if (url.pathname === textPath) {
      return { ...responseFor(url), node: { id: "text-1", path: currentChild().path } };
    }
    return responseFor(url);
  });
  // Keep the original and any replacement fetch in flight together. Counting
  // window.fetch also observes a request canceled before it reaches the route.
  await page.route((url) => url.pathname === folderPath, async (route) => {
    const response = currentFolder();
    await parentResponse;
    await route.fulfill({ json: response });
  });

  await page.goto("/");
  const recent = page.locator("[data-recent-list]");
  await recent.getByRole("button", { name: /^note\.md/ }).click();
  const editor = page.locator('[data-editor-group][data-active="true"]');
  await expect(editor).toContainText("Request budget");
  await expect(editor.getByRole("button", { name: "Edit", exact: true })).toBeEnabled();
  const tree = page.getByRole("tree", { name: "Files" });
  await expect(tree.getByRole("button", { name: "note.md", exact: true })).toBeVisible();
  await tree.getByRole("button", { name: "Collapse parent", exact: true }).click();
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await expect(inspector.getByText(folder.name, { exact: true })).toBeVisible();
  await expect(inspector).toContainText("Changes allowed");
  await waitForSync(1);
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1_000));

  for (const [index, nextLocked] of [true, false].entries()) {
    const before = await page.evaluate(() => ({ ...window.__lockFetchStarts }));
    locked = nextLocked;
    pendingChange = true;
    parentResponse = new Promise<void>((resolve) => { releaseParent = resolve; });
    try {
      await page.clock.runFor(30_001);
      await waitForSync(index + 2);
      await expect.poll(async () => {
        await page.clock.runFor(100);
        return editor.getByRole("button", { name: "Edit", exact: true }).isDisabled();
      }).toBe(nextLocked);
      const after = await page.evaluate(() => ({ ...window.__lockFetchStarts }));
      const delta = {
        parent: (after[folderPath] ?? 0) - (before[folderPath] ?? 0),
        child: (after[childPath] ?? 0) - (before[childPath] ?? 0),
        text: (after[textPath] ?? 0) - (before[textPath] ?? 0)
      };
      await test.info().attach(`folder-${nextLocked ? "lock" : "unlock"}-fetches`, {
        body: JSON.stringify({ before, after, delta, source: "window.fetch starts" }, null, 2),
        contentType: "application/json"
      });
      expect.soft(delta).toEqual({ parent: 1, child: 1, text: 0 });
      await expect.poll(async () => {
        await page.clock.runFor(100);
        return recent.getByRole("button", { name: /^note\.md/ })
          .locator("[data-node-lock-indicator]").count();
      }).toBe(nextLocked ? 1 : 0);
    } finally {
      releaseParent();
    }
    await expect.poll(async () => {
      await page.clock.runFor(100);
      return inspector.getByRole("switch", { name: "Lock changes" }).isChecked();
    }).toBe(nextLocked);
    await expect(inspector).toContainText(nextLocked ? "Locked here" : "Changes allowed");
  }
});

test("backs off idle usage polling with one owner while the desktop Space Library is open", async ({ page }) => {
  const requests = new Map<string, number>();
  await page.clock.install();
  const waitForUsage = await observeCompletedPolling(page, "/api/v1/me/usage");
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeJsonApi(page, (url) => {
    requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
    return responseFor(url);
  });

  await page.goto("/");
  await waitForUsage(1);

  await page.clock.runFor(10_000);
  await page.getByRole("button", { name: "Open space library" }).click();
  await expect(page.getByRole("heading", { name: /Spaces 1/ })).toBeVisible();
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1_000));

  for (const [elapsedMs, expectedRequests] of [
    [60_001, 2],
    [120_001, 3],
    [300_001, 4],
    [300_001, 5]
  ] as const) {
    await page.clock.runFor(elapsedMs);
    await waitForUsage(expectedRequests);
    expect(count(requests, "/api/v1/me/usage")).toBe(expectedRequests);
  }
});

declare global {
  interface Window {
    __completedPollingResponses: number;
    __startedPollingRequests: number;
    __lockFetchStarts: Record<string, number>;
  }
}

async function observeCompletedPolling(page: Page, path: string) {
  await page.addInitScript((pollPath) => {
    const state = window;
    state.__completedPollingResponses = 0;
    state.__startedPollingRequests = 0;
    const originalFetch = window.fetch;
    window.fetch = function (input, init) {
      if (typeof input === "string" && new URL(input, location.href).pathname === pollPath) {
        state.__startedPollingRequests += 1;
      }
      return originalFetch.call(window, input, init);
    };
    const channel = new MessageChannel();
    channel.port1.onmessage = () => { state.__completedPollingResponses += 1; };
    const originalJson = Response.prototype.json;
    Response.prototype.json = async function () {
      const body = await originalJson.call(this);
      if (new URL(this.url, location.href).pathname === pollPath) {
        // The following task runs after the consumer's promise chain updates
        // the query backoff and its interval. Request arrival is too early.
        channel.port2.postMessage(null);
      }
      return body;
    };
  }, path);
  return (expected: number) => expect.poll(() => page.evaluate(
    () => window.__completedPollingResponses
  )).toBe(expected);
}

function responseFor(url: URL) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
  if (url.pathname === "/api/v1/spaces") {
    return {
      spaces: [space],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/link-index`) {
    return {
      status: "idle",
      availability: { can_trigger: true, reason: null, retry_at: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: [textNode()],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return {
      nodes: [textNode()],
      page: { limit: 50, returned: 1, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-events`) {
    return {
      events: [],
      page: { limit: 50, returned: 0, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/text-1/reveal`) {
    return { ancestors: [], target: textNode() };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/text-1`) {
    return textNode();
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/text/text-1`) {
    return {
      node: { id: "text-1", path: "/note.md" },
      text: {
        node_id: "text-1",
        storage_format: "plain",
        content: "# Request budget",
        content_sha256: "sha-1",
        byte_len: 16,
        line_count: 1,
        start_line: 1,
        end_line: 1,
        returned_lines: 1,
        truncated: false,
        next_start_line: null,
        updated_by: me.account,
        updated_at: "2026-07-24T00:00:00Z"
      }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-sync`) {
    return {
      changes: [],
      next_after_id: 10,
      has_more: false,
      resync_required: false
    };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

function textNode(): RestNode {
  return {
    id: "text-1",
    space_id: space.id,
    parent_id: space.root_node_id,
    name: "note.md",
    kind: "text",
    path: "/note.md",
    sort_order: 0,
    metadata: {},
    external_access_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    has_children: false,
    effective_write_locked: false,
    content_sha256: "sha-1",
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-24T00:00:00Z",
    updated_at: "2026-07-24T00:00:00Z"
  };
}

function count(requests: Map<string, number>, path: string): number {
  return requests.get(path) ?? 0;
}

async function leaveAndReturnAfterStale(page: import("@playwright/test").Page) {
  await setPageVisibility(page, "hidden");
  await page.clock.runFor(5_001);
  await setPageVisibility(page, "visible");
}

async function setPageVisibility(
  page: import("@playwright/test").Page,
  visibilityState: "hidden" | "visible"
) {
  await page.evaluate(async (nextVisibilityState) => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: nextVisibilityState
    });
    document.dispatchEvent(new Event("visibilitychange"));
    window.dispatchEvent(new Event("visibilitychange"));
    await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
  }, visibilityState);
}

function totalRequests(requests: Map<string, number>): number {
  return [...requests.values()].reduce((total, requestCount) => total + requestCount, 0);
}

function restoredEditorRequestCount(requests: Map<string, number>): number {
  return [1, 2, 3].reduce((total, index) => (
    total
    + count(requests, `/api/v1/spaces/${space.id}/nodes/text-${index}`)
    + count(requests, `/api/v1/spaces/${space.id}/text/text-${index}`)
  ), 0);
}

function restoredEditorsResponseFor(url: URL, externalRevision: number) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
  if (url.pathname === "/api/v1/spaces") {
    return {
      spaces: [space],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
    };
  }

  const nodes = [1, 2, 3].map((index) => restoredTextNode(
    index,
    index === 2 ? externalRevision : 1
  ));
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: nodes,
      page: { limit: 100, returned: nodes.length, has_more: false, next_cursor: null }
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return {
      nodes,
      page: { limit: 50, returned: nodes.length, has_more: false, next_cursor: null }
    };
  }

  const nodeMatch = url.pathname.match(/\/nodes\/(text-[123])$/);
  if (nodeMatch) return nodes.find((node) => node.id === nodeMatch[1]);
  const textMatch = url.pathname.match(/\/text\/(text-[123])$/);
  if (textMatch) {
    const node = nodes.find((candidate) => candidate.id === textMatch[1]);
    if (!node) throw new Error(`Missing restored node: ${textMatch[1]}`);
    const updated = node.id === "text-2" && externalRevision === 2;
    const content = `# Request budget ${node.sort_order}${updated ? " updated" : ""}`;
    return {
      node: { id: node.id, path: node.path },
      text: {
        node_id: node.id,
        storage_format: "plain",
        content,
        content_sha256: node.content_sha256,
        byte_len: content.length,
        line_count: 1,
        start_line: 1,
        end_line: 1,
        returned_lines: 1,
        truncated: false,
        next_start_line: null,
        updated_by: me.account,
        updated_at: node.updated_at
      }
    };
  }
  throw new Error(`Unhandled restored-editor API request: ${url.pathname}${url.search}`);
}

function restoredTextNode(index: number, revision = 1): RestNode {
  return {
    ...textNode(),
    id: `text-${index}`,
    name: `note-${index}.md`,
    path: `/note-${index}.md`,
    sort_order: index,
    content_sha256: `sha-${index}-${revision}`,
    updated_at: `2026-07-24T00:00:0${revision}Z`
  };
}
