import type { Page, Request } from "@playwright/test";

type JsonApiResponder = (url: URL, request: Request) => unknown;

export async function routeJsonApi(
  page: Page,
  responseFor: JsonApiResponder
) {
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(responseFor(new URL(request.url()), request))
    });
  });
}
