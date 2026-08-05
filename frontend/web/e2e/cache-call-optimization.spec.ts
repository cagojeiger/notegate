import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { routeWorkbenchJsonApi } from "./support/linkIndex";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Performance fixture",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
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
    await page.clock.install();
    await page.addInitScript(() => {
      Math.random = () => 0.5;
    });
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await routeWorkbenchJsonApi(page, (url) => {
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
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/file-change-sync`)).toBe(1);
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    await expect.poll(() => count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);

    for (const [elapsedMs, expectedRequests] of [
      [30_001, 2],
      [60_001, 3],
      [120_001, 4],
      [300_001, 5],
      [300_001, 6]
    ] as const) {
      await page.clock.fastForward(elapsedMs);
      await expect.poll(() => count(
        requests,
        `/api/v1/spaces/${space.id}/file-change-sync`
      )).toBe(expectedRequests);
    }

    expect(count(requests, `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`)).toBe(1);
    expect(count(requests, `/api/v1/spaces/${space.id}/nodes`)).toBe(1);

    if (viewport.name === "desktop") {
      await page.context().setOffline(true);
      await page.context().setOffline(false);
      expect(count(requests, `/api/v1/spaces/${space.id}/file-change-sync`)).toBe(6);

      await page.clock.fastForward(30_001);
      await expect.poll(() => count(
        requests,
        `/api/v1/spaces/${space.id}/file-change-sync`
      )).toBe(7);
    }
  });
}

test("opens a recent node with one reveal request and no canonical node request", async ({ page }) => {
  const requests = new Map<string, number>();
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeWorkbenchJsonApi(page, (url) => {
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
  await routeWorkbenchJsonApi(page, (url) => responseFor(url));
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
  await routeWorkbenchJsonApi(page, (url) => {
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

test("backs off idle usage polling with one owner while the desktop Space Library is open", async ({ page }) => {
  const requests = new Map<string, number>();
  await page.clock.install();
  await page.setViewportSize({ width: 1440, height: 900 });
  await routeWorkbenchJsonApi(page, (url) => {
    requests.set(url.pathname, (requests.get(url.pathname) ?? 0) + 1);
    return responseFor(url);
  });

  await page.goto("/");
  await expect.poll(() => count(requests, "/api/v1/me/usage")).toBe(1);

  await page.clock.fastForward(10_000);
  await page.getByRole("button", { name: "Open space library" }).click();
  await expect(page.getByRole("heading", { name: /Spaces 1/ })).toBeVisible();

  for (const [elapsedMs, expectedRequests] of [
    [60_001, 2],
    [120_001, 3],
    [300_001, 4],
    [300_001, 5]
  ] as const) {
    await page.clock.fastForward(elapsedMs);
    await expect.poll(() => count(requests, "/api/v1/me/usage"))
      .toBe(expectedRequests);
  }
});

function responseFor(url: URL) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
  if (url.pathname === "/api/v1/spaces") {
    return {
      spaces: [space],
      page: { limit: 100, returned: 1, has_more: false, next_cursor: null }
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
    search_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    has_children: false,
    revision: 1,
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
  await page.clock.fastForward(5_001);
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
