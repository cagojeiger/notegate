import type { Page, Request } from "@playwright/test";

import { routeJsonApi, type JsonApiResponder } from "./api";

type LinkIndexResponder = (url: URL, request: Request) => unknown | undefined;

export async function routeWorkbenchJsonApi(
  page: Page,
  responseFor: JsonApiResponder,
  linkIndexResponseFor?: LinkIndexResponder
) {
  await routeJsonApi(page, (url, request) => {
    const defaultResponse = responseForLinkIndexRequest(url, request);
    if (defaultResponse === undefined) return responseFor(url, request);
    const override = linkIndexResponseFor?.(url, request);
    return override === undefined ? defaultResponse : override;
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
