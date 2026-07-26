import { expect, test } from "@playwright/test";

import type {
  BatchChildrenItem,
  Me,
  RestNode,
  Space
} from "../src/api/types";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Browsing fixture",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true },
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

  await routeApi(page, ({ url }) => {
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
  const target = node("target", "target.md", "text", folders.at(-1)?.id);
  let batchRequests = 0;
  let nestedChildrenRequests = 0;

  await routeApi(page, ({ url, method, postData }) => {
    if (method === "POST" && url.pathname.endsWith("/nodes:batchListChildren")) {
      batchRequests += 1;
      const parentIds = bodyParentIds(postData);
      return { results: parentIds.map((parentId) => readyResult(parentId, folders, target)) };
    }
    if (isChildren(url) && !url.pathname.includes(`/${space.root_node_id}/`)) {
      nestedChildrenRequests += 1;
      return childrenResponse(parentIdFrom(url), childrenFor(parentIdFrom(url), folders, target));
    }
    return browsingResponse(url, folders, target);
  });

  await page.goto("/");
  await page.locator("[data-recent-list]").getByRole("button", { name: target.name }).click();

  await expect(page.getByRole("tree", { name: "Files" }).getByRole("button", { name: target.name })).toBeVisible();
  expect(batchRequests).toBe(1);
  expect(nestedChildrenRequests).toBe(0);
});

test("a malformed batch response falls back to individual folder queries", async ({ page }) => {
  const folders = folderChain(4);
  const target = node("target", "target.md", "text", folders.at(-1)?.id);
  let batchRequests = 0;
  const requestedParents = new Set<string>();

  await routeApi(page, ({ url, method }) => {
    if (method === "POST" && url.pathname.endsWith("/nodes:batchListChildren")) {
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

type RouteRequest = {
  url: URL;
  method: string;
  postData: string | null;
};

async function routeApi(
  page: import("@playwright/test").Page,
  responseFor: (request: RouteRequest) => unknown
) {
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const body = responseFor({
      url,
      method: request.method(),
      postData: request.postData()
    });
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body)
    });
  });
}

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
    search_enabled: true,
    has_children: kind === "folder",
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
