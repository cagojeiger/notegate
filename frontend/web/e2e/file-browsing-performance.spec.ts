import { expect, test } from "@playwright/test";

import type {
  BatchChildrenItem,
  Me,
  RestNode,
  Space
} from "../src/api/types";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Browsing fixture",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_external_access_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-25T00:00:00Z",
  updated_at: "2026-07-25T00:00:00Z"
};

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

test("Recent loads a second page once, deduplicates the boundary, and renders hostile names literally", async ({ page }) => {
  const recentRequests: string[] = [];
  const hostileName = `"><script>window.__notegateInjected=1</script>.md`;

  await routeJsonApi(page, (url) => {
    if (isNodesList(url)) {
      recentRequests.push(url.search);
      if (url.searchParams.get("cursor") === "recent-cursor-1") {
        return {
          nodes: [node("recent-1", "first.md", "text"), node("recent-2", "second.md", "text")],
          page: pageInfo(2, false, null, 50)
        };
      }
      return {
        nodes: [node("recent-1", "first.md", "text"), node("hostile", hostileName, "text")],
        page: pageInfo(2, true, "recent-cursor-1", 50)
      };
    }
    return baseResponse(url, []);
  });

  await page.goto("/");

  const recent = page.locator("[data-recent-list]");
  await expect(recent.getByRole("button", { name: "second.md" })).toBeVisible();
  await expect(recent.getByRole("button", { name: "first.md" })).toHaveCount(1);
  await expect(recent).toContainText(hostileName);
  expect(await page.evaluate(() => Reflect.get(window, "__notegateInjected"))).toBeUndefined();
  expect(recentRequests).toHaveLength(2);
  expect(recentRequests[0]).toContain("view=summary");
  expect(recentRequests[1]).toContain("cursor=recent-cursor-1");
});

test("revealing a deeply nested recent node restores expanded folders with one batch request", async ({ page }) => {
  const folders = folderChain(10);
  const target = node("target", "target.md", "text", folders.at(-1)?.id, `${folders.at(-1)?.path}/target.md`);
  const preceding = Array.from({ length: 80 }, (_, index) =>
    node(`root-${index}`, `root-${index}.md`, "text")
  );
  let batchRequests = 0;
  let rootNextPageRequests = 0;
  let nestedChildrenRequests = 0;

  await routeJsonApi(page, (url, request) => {
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
      if (url.searchParams.has("cursor")) {
        rootNextPageRequests += 1;
        return childrenResponse(space.root_node_id, [folders[0]!]);
      }
      return { ...childrenResponse(space.root_node_id, preceding), page: pageInfo(80, true, "next-page", 100) };
    }
    if (request.method() === "POST" && url.pathname.endsWith("/nodes:batchListChildren")) {
      batchRequests += 1;
      const parentIds = bodyParentIds(request.postData());
      return { results: parentIds.map((parentId) => parentId === space.root_node_id
        ? {
            parent_id: parentId,
            status: "ready",
            parent: { id: parentId, path: "/" },
            children: preceding,
            page: pageInfo(80, true, "next-page", 100)
          }
        : readyResult(parentId, folders, target)) };
    }
    if (isChildren(url) && !url.pathname.includes(`/${space.root_node_id}/`)) {
      nestedChildrenRequests += 1;
      return childrenResponse(parentIdFrom(url), childrenFor(parentIdFrom(url), folders, target));
    }
    return browsingResponse(url, folders, target);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: target.name }).click();

  const tree = page.getByRole("tree", { name: "Files" });
  await expect(tree.getByRole("button", { name: target.name })).toBeVisible();
  await expect.poll(() => tree.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  expect(batchRequests).toBe(1);
  expect(rootNextPageRequests).toBe(1);
  expect(nestedChildrenRequests).toBe(0);
});

test("Recent loads the target's tree pages before scrolling to it", async ({ page }) => {
  const target = node("target", "target.md", "text");
  const firstPage = Array.from({ length: 80 }, (_, index) =>
    node(`root-${index}`, `root-${index}.md`, "text")
  );
  const secondPage = Array.from({ length: 80 }, (_, index) =>
    node(`root-${index + 80}`, `root-${index + 80}.md`, "text")
  );
  let nextPageRequests = 0;

  await routeJsonApi(page, (url) => {
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
      if (url.searchParams.get("cursor") === "next-page") {
        nextPageRequests += 1;
        return { ...childrenResponse(space.root_node_id, secondPage), page: pageInfo(80, true, "last-page", 100) };
      }
      if (url.searchParams.get("cursor") === "last-page") {
        nextPageRequests += 1;
        return { ...childrenResponse(space.root_node_id, [target]), page: pageInfo(1, false, null, 100) };
      }
      return { ...childrenResponse(space.root_node_id, firstPage), page: pageInfo(80, true, "next-page", 100) };
    }
    return browsingResponse(url, [], target);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: target.name }).click();

  const tree = page.getByRole("tree", { name: "Files" });
  await expect(tree.getByRole("button", { name: target.name })).toBeVisible();
  await expect.poll(() => tree.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  expect(nextPageRequests).toBe(2);
});

test("Recent uses the revealed path when its cached path is stale", async ({ page }) => {
  const folder = node("renamed", "renamed", "folder", space.root_node_id, "/renamed");
  const target = node("target", "target.md", "text", folder.id, "/renamed/target.md");
  const stale = { ...target, path: "/old/target.md" };
  const beforeFolder = Array.from({ length: 30 }, (_, index) =>
    node(`root-${index}`, `root-${index}.md`, "text")
  );
  const afterFolder = Array.from({ length: 69 }, (_, index) =>
    node(`root-${index + 30}`, `root-${index + 30}.md`, "text")
  );
  let rootNextPageRequests = 0;

  await routeJsonApi(page, (url) => {
    if (isNodesList(url)) return { nodes: [stale], page: pageInfo(1, false, null, 50) };
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
      if (url.searchParams.has("cursor")) {
        rootNextPageRequests += 1;
        return childrenResponse(space.root_node_id, []);
      }
      return {
        ...childrenResponse(space.root_node_id, [...beforeFolder, folder, ...afterFolder]),
        page: pageInfo(100, true, "next-page", 100)
      };
    }
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${folder.id}/children`) {
      return childrenResponse(folder.id, [target]);
    }
    return browsingResponse(url, [folder], target);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: target.name }).click();

  const tree = page.getByRole("tree", { name: "Files" });
  await expect(tree.getByRole("button", { name: target.name })).toBeVisible();
  expect(rootNextPageRequests).toBe(0);
});

test("an older Recent response does not replace a newer open node", async ({ page }) => {
  const first = node("first", "first.md", "text");
  const second = node("second", "second.md", "text");
  let releaseFirst!: () => void;
  let markFirstStarted!: () => void;
  const firstReleased = new Promise<void>((resolve) => { releaseFirst = resolve; });
  const firstStarted = new Promise<void>((resolve) => { markFirstStarted = resolve; });

  await routeJsonApi(page, (url) => {
    if (isNodesList(url)) return { nodes: [first, second], page: pageInfo(2, false, null, 50) };
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
      return childrenResponse(space.root_node_id, [first, second]);
    }
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${first.id}`) return first;
    return browsingResponse(url, [], second);
  });
  await page.route(`**/api/v1/spaces/${space.id}/nodes/${first.id}/reveal`, async (route) => {
    markFirstStarted();
    await firstReleased;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ancestors: [node(space.root_node_id, "", "folder", null)], target: first })
    });
  });

  await page.goto("/");
  const recent = page.locator("[data-recent-list]");
  await recent.getByRole("button", { name: first.name }).click();
  await firstStarted;
  await recent.getByRole("button", { name: second.name }).click();
  await expect(page.locator('[data-editor-group][data-active="true"]')).toContainText(second.name);

  const firstResponse = page.waitForResponse((response) => response.url().endsWith(`/nodes/${first.id}/reveal`));
  releaseFirst();
  await firstResponse;
  await expect(page.locator('[data-editor-group][data-active="true"]')).toContainText(second.name);
  await expect(page.getByRole("tree", { name: "Files" }).getByRole("treeitem", { name: second.name })).toHaveAttribute("aria-selected", "true");
});

test("mobile Files reveals the Recent target after reopening the sidebar", async ({ page }) => {
  const target = node("target", "target.md", "text");
  const firstPage = Array.from({ length: 80 }, (_, index) =>
    node(`root-${index}`, `root-${index}.md`, "text")
  );
  let nextPageRequests = 0;

  await page.setViewportSize({ width: 390, height: 844 });
  await routeJsonApi(page, (url) => {
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
      if (url.searchParams.has("cursor")) {
        nextPageRequests += 1;
        return childrenResponse(space.root_node_id, [target]);
      }
      return { ...childrenResponse(space.root_node_id, firstPage), page: pageInfo(80, true, "next-page", 100) };
    }
    return browsingResponse(url, [], target);
  });

  await page.goto("/");
  await page.getByRole("button", { name: "Toggle left sidebar" }).click();
  await page.locator("[data-recent-list]").getByRole("button", { name: target.name }).click();
  await expect(page.getByRole("tree", { name: "Files" })).toHaveCount(0);
  await page.getByRole("button", { name: "Toggle left sidebar" }).click();

  const tree = page.getByRole("tree", { name: "Files" });
  await expect(tree.getByRole("button", { name: target.name })).toBeVisible();
  await expect.poll(() => tree.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  expect(nextPageRequests).toBe(1);
});

test("a malformed batch response falls back to individual folder queries", async ({ page }) => {
  const folders = folderChain(4);
  const target = node("target", "target.md", "text", folders.at(-1)?.id);
  let batchRequests = 0;
  const requestedParents = new Set<string>();

  await routeJsonApi(page, (url, request) => {
    if (request.method() === "POST" && url.pathname.endsWith("/nodes:batchListChildren")) {
      batchRequests += 1;
      return { results: [] };
    }
    if (isChildren(url) && !url.pathname.includes(`/${space.root_node_id}/`)) {
      const parentId = parentIdFrom(url);
      requestedParents.add(parentId);
      return childrenResponse(parentId, childrenFor(parentId, folders, target));
    }
    return browsingResponse(url, folders, target);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: target.name }).click();

  await expect(page.getByRole("tree", { name: "Files" }).getByRole("button", { name: target.name })).toBeVisible();
  expect(batchRequests).toBe(1);
  expect(requestedParents).toEqual(new Set(folders.map((folder) => folder.id)));
});

function baseResponse(url: URL, rootChildren: RestNode[]) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
  if (url.pathname === "/api/v1/spaces") {
    return { spaces: [space], page: pageInfo(1, false, null, 100) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return childrenResponse(space.root_node_id, rootChildren);
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-sync`) {
    return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

function browsingResponse(url: URL, folders: RestNode[], target: RestNode) {
  if (isNodesList(url)) {
    return { nodes: [target], page: pageInfo(1, false, null, 50) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${target.id}`) return target;
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${target.id}/reveal`) {
    return {
      ancestors: [node(space.root_node_id, "", "folder", null), ...folders],
      target
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/text/${target.id}`) {
    return {
      node: { id: target.id, path: target.path },
      text: {
        storage_format: "markdown",
        content: "# Target",
        content_sha256: "sha-target",
        byte_len: 8,
        line_count: 1,
        updated_by: me.account,
        updated_at: target.updated_at
      }
    };
  }
  return baseResponse(url, folders.slice(0, 1));
}

function readyResult(
  parentId: string,
  folders: RestNode[],
  target: RestNode
): BatchChildrenItem {
  const parent = folders.find((folder) => folder.id === parentId);
  return {
    parent_id: parentId,
    status: "ready",
    parent: { id: parentId, path: parent?.path ?? "/" },
    children: childrenFor(parentId, folders, target),
    page: pageInfo(1, false, null, 100)
  };
}

function childrenFor(
  parentId: string,
  folders: RestNode[],
  target: RestNode
): RestNode[] {
  const index = folders.findIndex((folder) => folder.id === parentId);
  if (index < 0) return [];
  return index === folders.length - 1 ? [target] : [folders[index + 1]!];
}

function childrenResponse(parentId: string, children: RestNode[]) {
  return {
    parent: { id: parentId, path: "/" },
    children,
    page: pageInfo(children.length, false, null, 100)
  };
}

function bodyParentIds(postData: string | null): string[] {
  const body = JSON.parse(postData ?? "{}") as { parent_ids?: unknown };
  if (!Array.isArray(body.parent_ids) || body.parent_ids.some((id) => typeof id !== "string")) {
    throw new Error("Batch request did not contain string parent_ids");
  }
  return body.parent_ids;
}

function parentIdFrom(url: URL): string {
  const parentId = url.pathname.match(/\/nodes\/([^/]+)\/children$/)?.[1];
  if (!parentId) throw new Error(`Missing parent id in ${url.pathname}`);
  return parentId;
}

function isChildren(url: URL): boolean {
  return /\/nodes\/[^/]+\/children$/.test(url.pathname);
}

function isNodesList(url: URL): boolean {
  return url.pathname === `/api/v1/spaces/${space.id}/nodes`;
}

function folderChain(count: number): RestNode[] {
  return Array.from({ length: count }, (_, index) => {
    const parentId = index === 0 ? space.root_node_id : `folder-${index}`;
    return node(
      `folder-${index + 1}`,
      `folder-${index + 1}`,
      "folder",
      parentId,
      `/${Array.from({ length: index + 1 }, (__, pathIndex) => `folder-${pathIndex + 1}`).join("/")}`
    );
  });
}

function node(
  id: string,
  name: string,
  kind: RestNode["kind"],
  parentId: string | null = space.root_node_id,
  path = `/${name}`
): RestNode {
  return {
    id,
    space_id: space.id,
    parent_id: parentId,
    name,
    kind,
    path,
    sort_order: 0,
    metadata: {},
    external_access_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    has_children: kind === "folder",
    effective_write_locked: false,
    content_sha256: kind === "text" ? `sha-${id}` : undefined,
    created_by: me.account,
    updated_by: me.account,
    created_at: "2026-07-25T00:00:00Z",
    updated_at: "2026-07-25T00:00:00Z"
  };
}

function pageInfo(
  returned: number,
  hasMore: boolean,
  nextCursor: string | null,
  limit: number
) {
  return {
    limit,
    returned,
    has_more: hasMore,
    next_cursor: nextCursor
  };
}
