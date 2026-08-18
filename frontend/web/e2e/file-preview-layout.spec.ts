import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { expectNoAccessibilityViolations } from "./support/accessibility";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
  navigation_pinned: true,
  user_mcp_enabled: true,
  default_search_enabled: true,
  default_text_encryption_enabled: false,
  features: { text_encryption: true, write_lock: true },
  permission: "write",
  root_node_id: "root-1",
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

const me: Me = {
  account: { id: "user-1", kind: "user", display_name: "User" },
  user: { email: "user@example.com" },
  capabilities: { can_create_space: true, can_manage_agents: true }
};

const imageNode: RestNode = {
  id: "image-1",
  space_id: space.id,
  parent_id: space.root_node_id,
  name: "tall-preview.png",
  kind: "file",
  path: "/tall-preview.png",
  sort_order: 0,
  metadata: {},
  search_enabled: true,
  write_locked: false,
  write_lock_sources: [],
  has_children: false,
  effective_write_locked: false,
  byte_len: 1024,
  media_type: "image/png",
  detected_media_type: "image/png",
  preview_available: true,
  file_preview_kind: "image",
  encryption_mode: "none",
  created_by: me.account,
  updated_by: me.account,
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

const pdfNode: RestNode = {
  ...imageNode,
  id: "pdf-1",
  name: "preview-document.pdf",
  path: "/preview-document.pdf",
  media_type: "application/pdf",
  detected_media_type: "application/pdf",
  preview_available: false,
  file_preview_kind: "pdf"
};

const docxNode: RestNode = {
  ...imageNode,
  id: "docx-1",
  name: "preview-document.docx",
  path: "/preview-document.docx",
  byte_len: 1024,
  media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  detected_media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  preview_available: false,
  file_preview_kind: "docx"
};

for (const viewport of [
  { name: "desktop", width: 1440, height: 900, mobile: false },
  { name: "tablet", width: 900, height: 1024, mobile: false },
  { name: "mobile", width: 390, height: 844, mobile: true }
]) {
  test(`image, PDF, and DOCX previews stay inside the editor on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await mockFilePreviewApi(page);
    await page.goto("/");

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: imageNode.name }).first().click();
    await expect(page.getByRole("img", { name: imageNode.name })).toBeVisible();

    const download = page.getByRole("button", { name: "Download" });
    await expect(download).toBeVisible();
    await expectInsideActiveEditor(page, page.getByRole("img", { name: imageNode.name }));

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: pdfNode.name }).first().click();

    const pdfPreview = page.locator("[data-pdf-preview]");
    await expect(pdfPreview).toBeVisible();
    await expect(pdfPreview.locator("canvas")).toBeVisible();
    const pageInput = pdfPreview.getByRole("spinbutton", { name: "Page number" });
    await expect(pageInput).toHaveValue("1");
    await expect(pdfPreview.getByText("/ 2")).toBeVisible();
    await expectInsideActiveEditor(page, pdfPreview);

    await pdfPreview.getByRole("button", { name: "Next page" }).click();
    await expect(pageInput).toHaveValue("2");
    await pdfPreview.getByRole("button", { name: "Zoom in" }).click();
    await expect(pdfPreview.getByRole("button", { name: "Reset zoom" })).toHaveText("125%");
    await expectNoAccessibilityViolations(page);

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: docxNode.name }).first().click();

    const docxPreview = page.locator("[data-docx-preview]");
    await expect(docxPreview).toBeVisible();
    await expectInsideActiveEditor(page, docxPreview);
    const docxFrame = page.frameLocator(`iframe[title="${docxNode.name} DOCX pages"]`);
    await expect(docxFrame.getByText("NoteGate DOCX preview", { exact: true })).toBeVisible();
    await expect(docxFrame.getByText("한글 문서 미리보기", { exact: true })).toBeVisible();
  });
}

test("mobile Inspector details remain scrollable on a short viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 640 });
  await mockFilePreviewApi(page);
  await page.goto("/");

  await page.getByRole("button", { name: "Toggle left sidebar" }).click();
  await page.getByRole("button", { name: imageNode.name }).first().click();
  await page.getByRole("button", { name: "Toggle right sidebar" }).click();

  await expect(page.getByRole("separator", { name: "Resize Inspector" })).toHaveCount(0);

  const scrollRegion = page.getByTestId("node-inspector-scroll-region");
  await expect(scrollRegion).toBeVisible();
  const settingsHelpBox = await page.getByRole("button", { name: "About Settings" }).boundingBox();
  expect(settingsHelpBox?.height).toBeGreaterThanOrEqual(44);
  expect(settingsHelpBox?.width).toBeGreaterThanOrEqual(44);
  await expect.poll(
    async () => scrollRegion.evaluate((element) => element.scrollHeight > element.clientHeight)
  ).toBe(true);

  await scrollRegion.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(async () => scrollRegion.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
  await expect(scrollRegion.getByText("Settings", { exact: true })).toBeInViewport();
});

test("desktop Inspector can grow without pushing the preview outside its editor", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mockFilePreviewApi(page);
  await page.goto("/");

  await page.getByRole("button", { name: imageNode.name }).first().click();
  const preview = page.getByRole("img", { name: imageNode.name });
  await expect(preview).toBeVisible();

  const separator = page.getByRole("separator", { name: "Resize Inspector" });
  await expect(separator).toHaveAttribute("aria-valuenow", "320");
  const separatorBox = await separator.boundingBox();
  expect(separatorBox?.width).toBeGreaterThanOrEqual(24);
  if (!separatorBox) throw new Error("Inspector separator is not visible");

  await page.mouse.move(separatorBox.x + separatorBox.width / 2, separatorBox.y + 80);
  await page.mouse.down();
  await page.mouse.move(separatorBox.x + separatorBox.width / 2 - 80, separatorBox.y + 80);
  await page.mouse.up();

  await expect(separator).toHaveAttribute("aria-valuenow", "400");
  await expect(page.getByRole("complementary", { name: "Inspector" })).toHaveCSS("width", "400px");
  await expectInsideActiveEditor(page, preview);
});

async function mockFilePreviewApi(page: import("@playwright/test").Page) {
  const previewSvg = Buffer.from(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="4000"><rect width="1200" height="4000" fill="#ffffff"/><path d="M80 120h1040v3760H80z" fill="none" stroke="#185fc4" stroke-width="16"/></svg>'
  ).toString("base64");

  await page.route("http://storage.test/preview-document.pdf", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/pdf",
      headers: { "Access-Control-Allow-Origin": "*" },
      body: createPdf()
    });
  });
  await page.route("http://storage.test/preview-document.docx", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      headers: { "Access-Control-Allow-Origin": "*" },
      body: Buffer.from(DOCX_FIXTURE_BASE64, "base64")
    });
  });
  await routeJsonApi(page, (url) => responseFor(url, previewSvg));
}

function responseFor(url: URL, previewSvg: string) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/me/usage") return usageResponse(space, 2);
  if (url.pathname === "/api/v1/spaces") {
    return { spaces: [space], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: [imageNode, pdfNode, docxNode],
      page: pageInfo(3)
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return { nodes: [imageNode, pdfNode, docxNode], page: pageInfo(3) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/file-change-sync`) {
    return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${imageNode.id}`) {
    return imageNode;
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${imageNode.id}/reveal`) {
    return { ancestors: [], target: imageNode };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${pdfNode.id}`) {
    return pdfNode;
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${pdfNode.id}/reveal`) {
    return { ancestors: [], target: pdfNode };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${docxNode.id}`) {
    return docxNode;
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${docxNode.id}/reveal`) {
    return { ancestors: [], target: docxNode };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/files/${imageNode.id}/preview-url`) {
    return {
      url: `data:image/svg+xml;base64,${previewSvg}`,
      media_type: "image/png",
      expires_at: "2026-07-24T12:00:00Z"
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/files/${pdfNode.id}/pdf-preview-url`) {
    return {
      url: "http://storage.test/preview-document.pdf",
      media_type: "application/pdf",
      expires_at: "2026-07-24T12:00:00Z"
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/files/${docxNode.id}/docx-preview-url`) {
    return {
      url: "http://storage.test/preview-document.docx",
      media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
      expires_at: "2026-07-24T12:00:00Z"
    };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

async function expectInsideActiveEditor(
  page: import("@playwright/test").Page,
  content: import("@playwright/test").Locator
) {
  const editor = page.locator('[data-editor-group][data-active="true"]');
  const [editorBox, contentBox] = await Promise.all([editor.boundingBox(), content.boundingBox()]);
  expect(editorBox).not.toBeNull();
  expect(contentBox).not.toBeNull();
  expect(contentBox!.x).toBeGreaterThanOrEqual(editorBox!.x);
  expect(contentBox!.y).toBeGreaterThanOrEqual(editorBox!.y);
  expect(contentBox!.x + contentBox!.width).toBeLessThanOrEqual(editorBox!.x + editorBox!.width + 1);
  expect(contentBox!.y + contentBox!.height).toBeLessThanOrEqual(editorBox!.y + editorBox!.height + 1);
}

function createPdf() {
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>",
    stream("BT /F1 24 Tf 72 720 Td (Page 1) Tj ET"),
    stream("BT /F1 24 Tf 72 720 Td (Page 2) Tj ET")
  ];
  let pdf = "%PDF-1.4\n";
  const offsets = [0];
  objects.forEach((object, index) => {
    offsets.push(Buffer.byteLength(pdf));
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = Buffer.byteLength(pdf);
  pdf += `xref\n0 ${objects.length + 1}\n`;
  pdf += "0000000000 65535 f \n";
  pdf += offsets.slice(1).map((offset) => `${offset.toString().padStart(10, "0")} 00000 n \n`).join("");
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(pdf);
}

function stream(content: string) {
  return `<< /Length ${Buffer.byteLength(content)} >>\nstream\n${content}\nendstream`;
}

function pageInfo(returned: number) {
  return { limit: 100, returned, has_more: false, next_cursor: null };
}

const DOCX_FIXTURE_BASE64 = "UEsDBBQAAAAIAHV+El15bjPX6AAAAK0BAAATAAAAW0NvbnRlbnRfVHlwZXNdLnhtbH1QyU7DMBD9FWuuKHHggBCK0wPLETiUDxjZk8SqN3nc0v49Tlt6QIXjzFv1+tXeO7GjzDYGBbdtB4KCjsaGScHn+rV5AMEFg0EXAyk4EMNq6NeHRCyqNrCCuZT0KCXrmTxyGxOFiowxeyz1zJNMqDc4kbzrunupYygUSlMWDxj6Zxpx64p42df3qUcmxyCeTsQlSwGm5KzGUnG5C+ZXSnNOaKvyyOHZJr6pBJBXExbk74Cz7r0Ok60h8YG5vKGvLPkVs5Em6q2vyvZ/mys94zhaTRf94pZy1MRcF/euvSAebfjpL49zD99QSwMEFAAAAAgAdX4SXZv9N+qtAAAAKQEAAAsAAABfcmVscy8ucmVsc43POw7CMAwG4KtE3mlaBoRQ0y4IqSsqB7ASN61oHkrCo7cnAwNFDIy2f3+W6/ZpZnanECdnBVRFCYysdGqyWsClP232wGJCq3B2lgQsFKFt6jPNmPJKHCcfWTZsFDCm5A+cRzmSwVg4TzZPBhcMplwGzT3KK2ri27Lc8fBpwNpknRIQOlUB6xdP/9huGCZJRydvhmz6ceIrkWUMmpKAhwuKq3e7yCzwpuarF5sXUEsDBBQAAAAIAHV+El1HjNXVEwEAAJMBAAARAAAAd29yZC9kb2N1bWVudC54bWxtUE1Lw0AQ/SvL3u2mpUoJSXtQ9OQHqOB1m0yTQHZn2R0b68mDf8Fbz54KCnrIb2rqf3C3pRTEy5t5M/PeMJNMnlTN5mBdhTrl/V7EGegM80oXKb+/Oz8aceZI6lzWqCHlC3B8Mk6aOMfsUYEm5g20i5uUl0QmFsJlJSjpemhA+94MrZLkqS1EgzY3FjNwzvurWgyi6EQoWWkeLKeYL0I0AWwAGl8hwYUkYGfXpw/MWJhX0CQitALaLZq/qp+35bp9Yd2q3bwuWffRdu+r7ut73X7+K3WQ0c1WaorbZ9aEa/qDwdA/o4lLnx+PfC52A5fS+iqh8fXhbsRWRUkHOkUiVAdew2zfFdul+31if7M4/HP8C1BLAQIUAxQAAAAIAHV+El15bjPX6AAAAK0BAAATAAAAAAAAAAAAAACAAQAAAABbQ29udGVudF9UeXBlc10ueG1sUEsBAhQDFAAAAAgAdX4SXZv9N+qtAAAAKQEAAAsAAAAAAAAAAAAAAIABGQEAAF9yZWxzLy5yZWxzUEsBAhQDFAAAAAgAdX4SXUeM1dUTAQAAkwEAABEAAAAAAAAAAAAAAIAB7wEAAHdvcmQvZG9jdW1lbnQueG1sUEsFBgAAAAADAAMAuQAAADEDAAAAAA==";
