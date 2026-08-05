import type { Page, Request } from "@playwright/test";

export type JsonApiResponder = (url: URL, request: Request) => unknown;

export async function routeJsonApi(
  page: Page,
  responseFor: JsonApiResponder
) {
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(responseFor(url, request))
    });
  });
}
