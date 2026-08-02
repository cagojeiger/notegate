import type { Page, Request } from "@playwright/test";

type JsonApiResponder = (url: URL, request: Request) => unknown;

export async function routeJsonApi(
  page: Page,
  responseFor: JsonApiResponder
) {
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const linkIndexResponse = responseForLinkIndexRequest(url, request);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(linkIndexResponse ?? responseFor(url, request))
    });
  });
}

export function defaultLinkIndexState(spaceId: string) {
  return {
    space_id: spaceId,
    desired_generation: 0,
    applied_generation: 0,
    status: "ready",
    freshness: "current",
    last_indexed_at: null
  };
}

function responseForLinkIndexRequest(url: URL, request: Request): unknown | undefined {
  if (request.method() !== "GET") return undefined;

  const stateMatch = url.pathname.match(/^\/api\/v1\/spaces\/([^/]+)\/link-index$/);
  if (stateMatch) return defaultLinkIndexState(decodeURIComponent(stateMatch[1]));

  const nodeLinksMatch = url.pathname.match(/^\/api\/v1\/spaces\/([^/]+)\/nodes\/[^/]+\/links$/);
  if (!nodeLinksMatch) return undefined;

  return {
    index: defaultLinkIndexState(decodeURIComponent(nodeLinksMatch[1])),
    outgoing_count: 0,
    incoming_count: 0,
    broken_count: 0,
    outgoing: [],
    incoming: [],
    outgoing_truncated: false,
    incoming_truncated: false
  };
}
