import { expect, test } from "@playwright/test";

import type { Me, RestNode, Space } from "../src/api/types";
import { expectNoAccessibilityViolations } from "./support/accessibility";

const space: Space = {
  id: "space-1",
  name: "Daily",
  sort_order: 0,
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
  has_children: false,
  byte_len: 1024,
  media_type: "image/png",
  detected_media_type: "image/png",
  preview_available: true,
  encryption_mode: "none",
  created_by: me.account,
  updated_by: me.account,
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z"
};

const pdfNode: RestNode = {
  ...imageNode,
  id: "pdf-1",
  name: "document.pdf",
  path: "/document.pdf",
  media_type: "application/octet-stream",
  detected_media_type: "application/pdf"
};

const fileViewports = [
  { name: "desktop", width: 1440, height: 900, mobile: false },
  { name: "tablet", width: 900, height: 1024, mobile: false },
  { name: "mobile", width: 390, height: 844, mobile: true }
] as const;

const pdfViewports = [
  { name: "desktop", width: 1440, height: 900, mobile: false, minimumWidth: 480 },
  { name: "tablet-min", width: 768, height: 1024, mobile: false, minimumWidth: 480 },
  { name: "tablet", width: 900, height: 1024, mobile: false, minimumWidth: 480 },
  { name: "desktop-edge", width: 1024, height: 900, mobile: false, minimumWidth: 480 },
  { name: "mobile", width: 390, height: 844, mobile: true, minimumWidth: 320 }
] as const;

for (const viewport of fileViewports) {
  test(`tall file previews stay inside the editor on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await mockFilePreviewApi(page);
    await page.goto("/");

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: imageNode.name }).first().click();
    await expect(page.getByRole("img", { name: imageNode.name })).toBeVisible();

    const scroller = page.locator("[data-file-detail-scroll]");
    await expect(scroller).toBeVisible();
    await expect.poll(() => scroller.evaluate((element) => ({
      overflowY: getComputedStyle(element).overflowY,
      scrollable: element.scrollHeight > element.clientHeight
    }))).toEqual({ overflowY: "auto", scrollable: true });

    const download = page.getByRole("button", { name: "Download" });
    await download.scrollIntoViewIfNeeded();
    await expectNoAccessibilityViolations(page);
    const [scrollerBox, downloadBox] = await Promise.all([scroller.boundingBox(), download.boundingBox()]);
    expect(scrollerBox).not.toBeNull();
    expect(downloadBox).not.toBeNull();
    expect(downloadBox!.y + downloadBox!.height).toBeLessThanOrEqual(scrollerBox!.y + scrollerBox!.height + 1);

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
      const spaces = page.getByRole("navigation", { name: "Spaces" });
      const spacesBox = await spaces.boundingBox();
      expect(spacesBox).not.toBeNull();
      expect(await page.evaluate(({ x, y }) => {
        const target = document.elementFromPoint(x, y);
        return Boolean(target?.closest('nav[aria-label="Spaces"]'));
      }, {
        x: spacesBox!.x + spacesBox!.width / 2,
        y: spacesBox!.y + 8
      })).toBe(true);
    }
  });
}

for (const viewport of pdfViewports) {
  test(`PDF previews stay inside the editor on ${viewport.name}`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await mockFilePreviewApi(page);
    await page.goto("/");

    if (viewport.mobile) {
      await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    }
    await page.getByRole("button", { name: pdfNode.name }).first().click();

    const preview = page.getByRole("region", { name: `PDF preview: ${pdfNode.name}` });
    await expect(preview).toBeVisible();
    await expect(preview.locator("embedpdf-container")).toHaveAttribute("data-color-scheme", /light|dark/);
    await expect(preview.locator('img[src^="blob:"]').first()).toBeVisible();
    const scroller = page.locator("[data-file-detail-scroll]");
    const [previewBox, scrollerBox] = await Promise.all([
      preview.boundingBox(),
      scroller.boundingBox()
    ]);
    expect(previewBox).not.toBeNull();
    expect(scrollerBox).not.toBeNull();
    expect(previewBox!.width).toBeGreaterThanOrEqual(viewport.minimumWidth);
    expect(previewBox!.height).toBeGreaterThan(300);
    expect(previewBox!.x).toBeGreaterThanOrEqual(scrollerBox!.x);
    expect(previewBox!.x + previewBox!.width).toBeLessThanOrEqual(
      scrollerBox!.x + scrollerBox!.width + 1
    );

    const download = page.getByRole("button", { name: "Download" });
    await download.scrollIntoViewIfNeeded();
    await expect(download).toBeVisible();
    await expectNoAccessibilityViolations(page);

    if (viewport.name === "desktop") {
      const initialScheme = await preview.locator("embedpdf-container").getAttribute("data-color-scheme");
      await page.getByRole("button", { name: "Toggle theme" }).click();
      await expect(preview.locator("embedpdf-container")).toHaveAttribute(
        "data-color-scheme",
        initialScheme === "dark" ? "light" : "dark"
      );
    }
  });
}

test("PDF reading mode focuses the active editor when split panes would be too narrow", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 1024 });
  await mockFilePreviewApi(page);
  await page.goto("/");

  await page.getByRole("button", { name: imageNode.name }).first().click();
  await page.getByRole("button", { name: pdfNode.name }).first().click({ button: "right" });
  await page.getByRole("menu").getByRole("button", { name: "Open in new group" }).click();

  const preview = page.getByRole("region", { name: `PDF preview: ${pdfNode.name}` });
  await expect(preview.locator('img[src^="blob:"]').first()).toBeVisible();
  await expect(page.locator("[data-editor-group]:visible")).toHaveCount(1);
  expect((await preview.boundingBox())?.width).toBeGreaterThanOrEqual(480);
});

async function mockFilePreviewApi(page: import("@playwright/test").Page) {
  const previewSvg = Buffer.from(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="4000"><rect width="1200" height="4000" fill="#ffffff"/><path d="M80 120h1040v3760H80z" fill="none" stroke="#185fc4" stroke-width="16"/></svg>'
  ).toString("base64");
  const previewPdf = createPreviewPdf().toString("base64");

  await page.route("**/api/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const response = responseFor(url, previewSvg, previewPdf);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(response) });
  });
}

function createPreviewPdf(): Buffer {
  const firstPage = [
    "q",
    "0.094 0.373 0.769 rg",
    "72 648 468 72 re f",
    "Q",
    "BT",
    "/F1 28 Tf",
    "1 1 1 rg",
    "96 684 Td",
    "(NoteGate PDF Preview) Tj",
    "ET",
    "BT",
    "/F1 16 Tf",
    "0.09 0.13 0.17 rg",
    "72 596 Td",
    "(A polished in-app reading experience) Tj",
    "0 -36 Td",
    "/F1 12 Tf",
    "(Search, zoom, page navigation, print, and fullscreen are built in.) Tj",
    "0 -24 Td",
    "(The viewer follows the NoteGate light and dark themes.) Tj",
    "0 -24 Td",
    "(PDF bytes stay inside the browser and no external fonts are requested.) Tj",
    "ET"
  ].join("\n");
  const secondPage = [
    "BT",
    "/F1 24 Tf",
    "0.09 0.13 0.17 rg",
    "72 690 Td",
    "(Page 2) Tj",
    "0 -42 Td",
    "/F1 13 Tf",
    "(Page navigation is rendered by PDFium inside NoteGate.) Tj",
    "ET"
  ].join("\n");
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>",
    `<< /Length ${Buffer.byteLength(firstPage, "latin1")} >>\nstream\n${firstPage}\nendstream`,
    `<< /Length ${Buffer.byteLength(secondPage, "latin1")} >>\nstream\n${secondPage}\nendstream`,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
  ];
  const offsets = [0];
  let pdf = "%PDF-1.4\n";

  for (const [index, object] of objects.entries()) {
    offsets.push(Buffer.byteLength(pdf, "latin1"));
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  }

  const xrefOffset = Buffer.byteLength(pdf, "latin1");
  pdf += `xref\n0 ${objects.length + 1}\n`;
  pdf += "0000000000 65535 f \n";
  pdf += offsets.slice(1).map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`).join("");
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xrefOffset}\n%%EOF\n`;

  return Buffer.from(pdf, "latin1");
}

function responseFor(url: URL, previewSvg: string, previewPdf: string) {
  if (url.pathname === "/api/v1/me") return me;
  if (url.pathname === "/api/v1/spaces") {
    return { spaces: [space], page: pageInfo(1) };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${space.root_node_id}/children`) {
    return {
      parent: { id: space.root_node_id, path: "/" },
      children: [imageNode, pdfNode],
      page: pageInfo(2)
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/nodes`) {
    return { nodes: [imageNode, pdfNode], page: pageInfo(2) };
  }
  for (const node of [imageNode, pdfNode]) {
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${node.id}`) {
      return node;
    }
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${node.id}/reveal`) {
      return { ancestors: [], target: node };
    }
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/files/${imageNode.id}/preview-url`) {
    return {
      url: `data:image/svg+xml;base64,${previewSvg}`,
      media_type: "image/png",
      expires_at: "2026-07-24T12:00:00Z"
    };
  }
  if (url.pathname === `/api/v1/spaces/${space.id}/files/${pdfNode.id}/preview-url`) {
    return {
      url: `data:application/pdf;base64,${previewPdf}`,
      media_type: "application/pdf",
      expires_at: "2026-07-24T12:00:00Z"
    };
  }
  throw new Error(`Unhandled API request: ${url.pathname}${url.search}`);
}

function pageInfo(returned: number) {
  return { limit: 100, returned, has_more: false, next_cursor: null };
}
