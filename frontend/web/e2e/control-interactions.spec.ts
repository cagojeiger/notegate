import { expect, test, type Locator, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

import type { RestNode } from "../src/api/types";
import { makeRestNode, makeSpace } from "../src/test/fixtures";
import { routeJsonApi } from "./support/api";
import { usageResponse } from "./support/usage";
import { openSettings, showSettingsInspector, closeSettingsInspector } from "./support/settings";

async function mockWorkbench(page: Page, nodeCount = 1, nodeOverrides: Partial<RestNode> = {}) {
  let space = makeSpace();
  const node = makeRestNode(nodeOverrides);
  const nodes = [node, ...Array.from({ length: nodeCount - 1 }, (_, index) => makeRestNode({ id: `node-extra-${index}`, name: `note-${index}.md`, path: `/note-${index}.md` }))];
  const meta = { limit: 50, returned: 0, has_more: false, next_cursor: null };
  await routeJsonApi(page, (url, request) => {
    if (url.pathname === "/api/v1/me") return { account: { id: "user-1", kind: "user", display_name: "User" }, user: { email: "user@example.com" }, capabilities: { can_create_space: true, can_manage_agents: true } };
    if (url.pathname === "/api/v1/me/usage") return usageResponse(space);
    if (url.pathname === "/api/v1/me/sessions") return { sessions: [] };
    if (url.pathname === "/api/v1/me/oauth-grants") return { grants: [] };
    if (url.pathname === "/api/v1/spaces") return { spaces: [space], page: meta };
    if (url.pathname === `/api/v1/spaces/${space.id}` && request.method() === "PATCH") {
      space = { ...space, ...request.postDataJSON() };
      return space;
    }
    if (url.pathname === "/api/v1/agents") return { agents: [], page: meta };
    if (url.pathname === "/api/v1/me/trash") return { items: [], page: meta };
    if (url.pathname === "/api/v1/me/audit-events") return { events: [], page: meta };
    if (url.pathname.endsWith("/file-change-events")) return { events: [], page: meta };
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${node.id}`) return node;
    if (url.pathname === `/api/v1/spaces/${space.id}/nodes/${node.id}/reveal`) return { ancestors: [], target: node };
    if (url.pathname === `/api/v1/spaces/${space.id}/text/${node.id}`) return {
      node: { id: node.id, path: node.path },
      text: { node_id: node.id, storage_format: "plain", content: node.name.endsWith(".json") ? '{"title":"Example"}' : "# Example", content_sha256: "fixture", byte_len: 19, line_count: 1, start_line: 1, end_line: 1, returned_lines: 1, truncated: false, next_start_line: null, updated_by: node.updated_by, updated_at: node.updated_at }
    };
    if (url.pathname === `/api/v1/spaces/${space.id}/files/${node.id}/preview-url`) return {
      url: 'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="16" height="16"%3E%3Crect width="16" height="16" fill="blue"/%3E%3C/svg%3E', media_type: "image/svg+xml", expires_at: "2099-01-01T00:00:00Z"
    };
    if (url.pathname.endsWith("/link-index")) return { status: "idle", availability: { can_trigger: true, reason: null, retry_at: null } };
    if (url.pathname.endsWith("/children")) return { parent: { id: space.root_node_id, path: "/" }, children: nodes, page: meta };
    if (url.pathname.endsWith("/nodes")) return { nodes: [], page: meta };
    if (url.pathname.endsWith("/file-change-sync")) return { changes: [], next_after_id: 0, has_more: false, resync_required: false };
    throw new Error(`Unhandled API request: ${request.method()} ${url.pathname}`);
  });
  return node;
}

async function mockAgentManagement(page: Page) {
  await mockWorkbench(page);
  let agents = [{ id: "review-agent", name: "Review agent", owner_user_id: "user-1" }];
  await page.route("**/api/v1/agents**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (request.method() === "DELETE") {
      agents = agents.filter((agent) => !path.endsWith(`/${agent.id}`));
      return route.fulfill({ status: 204 });
    }
    if (request.method() === "POST") {
      const agent = { id: "replacement-agent", name: request.postDataJSON().name as string, owner_user_id: "user-1" };
      agents.push(agent);
      return route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify(agent) });
    }
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(path.endsWith("/keys") ? { keys: [] } : { agents }) });
  });
  await page.route("**/api/v1/spaces/*/agents?*", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ connections: [] }) }));
}

test("mobile agent creation stays in the body after deleting an inspected agent", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockAgentManagement(page);
  await page.goto("/");
  const view = await openSettings(page, "Agents");
  await view.getByRole("button", { name: "Select Review agent", exact: true }).click();
  await view.getByRole("button", { name: "Open agent actions", exact: true }).click();
  const inspector = page.getByRole("dialog", { name: "Agents Inspector", exact: true });
  await inspector.getByRole("button", { name: "Delete Review agent", exact: true }).click();
  await inspector.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect(inspector).toHaveCount(0);
  await view.getByRole("button", { name: "Add agent", exact: true }).click();
  await view.getByRole("textbox", { name: "New agent name", exact: true }).fill("Replacement");
  await view.getByRole("button", { name: "Create", exact: true }).click();
  await expect(view.locator("[data-settings-heading] h2")).toHaveText("Replacement");
  await expect(inspector).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Toggle settings inspector", exact: true })).toHaveAttribute("aria-pressed", "false");
  await expect(view.getByRole("combobox", { name: "Daily permission", exact: true })).toBeInViewport();
});

for (const width of [1440, 390]) {
  test(`agent deletion confirmation retains keyboard focus at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await mockAgentManagement(page);
    await page.goto("/");
    const view = await openSettings(page, "Agents");
    await view.getByRole("button", { name: "Select Review agent", exact: true }).click();
    await view.getByRole("button", { name: "Open agent actions", exact: true }).click();
    const remove = view.getByRole("button", { name: "Delete Review agent", exact: true });
    await remove.click();
    const cancel = view.getByRole("button", { name: "Cancel", exact: true });
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(remove).toBeFocused();
  });
}

for (const width of [1440, 390, 320]) {
  test(`settings tabs retain keyboard focus across Account at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await mockWorkbench(page);
    await page.goto("/");
    const view = await openSettings(page);
    const tabs = view.getByRole("tablist", { name: "Settings sections", exact: true });
    await tabs.getByRole("tab", { name: "General", exact: true }).focus();
    for (const [key, label] of [
      ["ArrowRight", "Account"], ["ArrowRight", "Connections"],
      ["ArrowLeft", "Account"], ["ArrowLeft", "General"],
      ["End", "Trash"], ["Home", "General"]
    ]) {
      await page.keyboard.press(key);
      const active = tabs.getByRole("tab", { name: label, exact: true });
      await expect(active).toHaveAttribute("aria-selected", "true");
      await expect(active).toBeFocused();
      await expect(active).toBeInViewport({ ratio: 1 });
    }
  });

  test(`settings preserves an unsaved document and restores focus at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const node = await mockWorkbench(page);
    await page.goto("/");
    if (width < 768) await page.getByRole("button", { name: "Toggle left sidebar" }).click();
    await page.getByRole("button", { name: node.name, exact: true }).click();
    const edit = page.getByRole("button", { name: "Edit", exact: true });
    if (await edit.isVisible()) await edit.click();
    else {
      await page.getByRole("button", { name: "More actions", exact: true }).click();
      await page.getByRole("menuitem", { name: "Edit", exact: true }).click();
    }
    const editor = page.getByRole("textbox", { name: "Edit text content", includeHidden: true });
    await editor.fill("Keep my unsaved settings-review draft");
    const view = await openSettings(page);
    await expect(editor).toBeAttached();
    await expect(editor).toBeHidden();
    await expect(page.getByRole("button", { name: "History", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Trash", exact: true })).toHaveCount(0);
    for (const tab of ["General", "Account"]) {
      await view.getByRole("tab", { name: tab, exact: true }).click();
      await expect(view.getByRole("tabpanel", { name: tab, exact: true })).toBeVisible();
      const result = await new AxeBuilder({ page }).include("[data-settings-view]").withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
      expect(result.violations).toEqual([]);
      expect(await view.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
    }
    await showSettingsInspector(page);
    const inspectorTrigger = view.getByRole("button", { name: "Open session actions", exact: true });
    await expect(inspectorTrigger).toHaveAttribute("aria-expanded", "true");
    const controlsId = await inspectorTrigger.getAttribute("aria-controls");
    expect(await view.locator(".ng-settings-inspector-frame").getAttribute("id")).toBe(controlsId);
    if (width < 768) {
      const sheet = page.getByRole("dialog", { name: "Account Inspector" });
      await expect(sheet.getByRole("button", { name: "Close", exact: true })).toBeFocused();
      await page.keyboard.press("Escape");
      await expect(sheet).toBeHidden();
      await expect(page.getByRole("button", { name: "Toggle settings inspector" })).toBeFocused();
    } else {
      await view.getByRole("button", { name: "Close settings inspector" }).click();
      await expect(view.getByRole("tabpanel", { name: "Account", exact: true })).toBeVisible();
    }
    await expect(inspectorTrigger).toHaveAttribute("aria-expanded", "false");
    await closeSettingsInspector(page);
    await view.getByRole("button", { name: "Close settings", exact: true }).click();
    await expect(editor).toBeVisible();
    await expect(editor).toHaveValue("Keep my unsaved settings-review draft");
    await expect(page.getByRole("button", { name: "Settings", exact: true })).toBeFocused();
  });
}

for (const width of [1440, 390]) {
  test(`Space Library and Settings share tabs, gutters and control geometry at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await mockWorkbench(page);
    await page.goto("/");
    await page.getByRole("button", { name: "Open space library", exact: true }).click();
    const spaces = page.getByRole("tab", { name: "Spaces 1", exact: true });
    await expect(spaces).toHaveAttribute("aria-selected", "true");
    const workspaceHeader = page.locator("[data-panel-header]").filter({ has: spaces });
    const workspaceBounds = await workspaceHeader.boundingBox();
    const addRadius = await page.getByRole("button", { name: "Create space", exact: true })
      .evaluate((element) => getComputedStyle(element).borderRadius);
    const workspacePadding = await page.getByRole("tabpanel", { name: "Spaces 1", exact: true })
      .evaluate((element) => { const style = getComputedStyle(element); return [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft]; });

    const settings = await openSettings(page);
    const general = settings.getByRole("tab", { name: "General", exact: true });
    await expect(general).toHaveAttribute("aria-selected", "true");
    const settingsHeader = settings.locator("[data-panel-header]").filter({ has: page.getByRole("tab", { name: "General", exact: true }) });
    const settingsBounds = await settingsHeader.boundingBox();
    expect(workspaceBounds).not.toBeNull();
    expect(settingsBounds).not.toBeNull();
    expect({ x: settingsBounds!.x, y: settingsBounds!.y, height: settingsBounds!.height })
      .toEqual({ x: workspaceBounds!.x, y: workspaceBounds!.y, height: workspaceBounds!.height });
    expect(await settings.getByRole("tabpanel", { name: "General", exact: true })
      .evaluate((element) => { const style = getComputedStyle(element); return [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft]; }))
      .toEqual(workspacePadding);

    const reset = settings.getByRole("button", { name: "Reset", exact: true });
    const buttonHeight = await reset.evaluate((element) => element.getBoundingClientRect().height);
    expect(buttonHeight).toBe(width < 768 ? 44 : 28);
    expect(await reset.evaluate((element) => getComputedStyle(element).borderRadius)).toBe(addRadius);
    await settings.getByRole("tab", { name: "History", exact: true }).click();
    expect(await settings.getByRole("combobox", { name: "History type", exact: true })
      .evaluate((element) => element.getBoundingClientRect().height)).toBe(buttonHeight);

    await settings.getByRole("button", { name: "Close settings", exact: true }).click();
    await page.getByRole("button", { name: "Create space", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "New space", exact: true });
    expect(await dialog.getByRole("textbox", { name: "Space name", exact: true })
      .evaluate((element) => element.getBoundingClientRect().height)).toBe(buttonHeight);
  });
}

test("settings sections remain discoverable and keep the selected tab visible after resizing", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockWorkbench(page);
  await page.goto("/");
  const view = await openSettings(page);
  const more = view.getByRole("button", { name: "More settings sections", exact: true });
  await expect(more).toBeVisible();
  await more.click();
  const trash = view.getByRole("tab", { name: "Trash", exact: true });
  await trash.click();
  for (const width of [320, 844, 1440, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await expect(trash).toHaveAttribute("aria-selected", "true");
    await expect(trash).toBeInViewport({ ratio: 1 });
    expect(await view.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  }
});

for (const width of [1440, 390]) {
  test(`settings exposes primary controls and opens only contextual Inspectors at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await mockWorkbench(page);
    await page.goto("/");
    const view = await openSettings(page);
    let headingOrigin: { x: number; y: number } | undefined;
    for (const [tab, label] of [
      ["General", null], ["Account", "Open session actions"], ["Connections", null],
      ["Agents", null], ["History", null], ["Trash", "Open trash actions"]
    ] as const) {
      await view.getByRole("tab", { name: tab, exact: true }).click();
      await closeSettingsInspector(page);
      const toggle = page.getByRole("button", { name: "Toggle settings inspector" });
      if (await toggle.getAttribute("aria-pressed") === "true") await toggle.click();
      const heading = await view.locator("[data-settings-heading]").boundingBox();
      if (!heading) throw new Error("Missing settings heading");
      headingOrigin ??= { x: heading.x, y: heading.y };
      expect({ x: heading.x, y: heading.y }, `${tab} heading alignment`).toEqual(headingOrigin);
      await expect(view.getByRole("tab", { name: tab!, exact: true })).toBeInViewport({ ratio: 1 });
      if (!label) {
        await expect(toggle).toBeDisabled();
        await expect(view.getByRole("complementary")).toHaveCount(0);
        continue;
      }
      const opener = view.getByRole("button", { name: label, exact: true });
      await opener.click();
      const inspector = view.getByRole(width < 768 ? "dialog" : "complementary", { name: `${tab} Inspector`, exact: true });
      await expect(inspector).toBeVisible();
      if (width < 768) {
        await page.keyboard.press("Escape");
        await expect(inspector).toBeHidden();
        await expect(opener).toBeFocused();
      } else await toggle.click();
    }
  });
}

for (const width of [1440, 390]) {
  test(`Connections switches from the body and preserves keyboard focus at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await mockWorkbench(page);
    await page.goto("/");
    const view = await openSettings(page, "Connections");
    const navigation = view.getByRole("tablist", { name: "Connection views" });
    await navigation.getByRole("tab", { name: "Connection guide", exact: true }).focus();
    for (const [key, name, title] of [
      ["ArrowRight", "CLI", "CLI connections"], ["ArrowRight", "MCP", "MCP connections"],
      ["Home", "Connection guide", "Connections"], ["End", "MCP", "MCP connections"],
      ["ArrowLeft", "CLI", "CLI connections"], ["ArrowLeft", "Connection guide", "Connections"]
    ]) {
      await page.keyboard.press(key);
      const target = navigation.getByRole("tab", { name, exact: true });
      await expect(target).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(target).toBeFocused();
      await expect(target).toHaveAttribute("aria-selected", "true");
      await expect(view.getByRole("heading", { name: title, exact: true })).toBeVisible();
      await expect(page.getByRole("dialog", { name: "Connections Inspector" })).toHaveCount(0);
    }
  });
}

test("mobile History exposes its type and refresh without opening the Inspector", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockWorkbench(page);
  await page.goto("/");
  const view = await openSettings(page, "History");
  const type = view.getByRole("combobox", { name: "History type", exact: true });
  await expect(type).toBeInViewport();
  await type.focus();
  await type.selectOption("audit");
  await expect(view.getByText("No audit events.", { exact: true })).toBeVisible();
  await expect(type).toBeFocused();
  await expect(view.getByRole("button", { name: "Refresh", exact: true })).toBeInViewport();
  await expect(page.getByRole("dialog", { name: "History Inspector" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Toggle settings inspector" })).toBeDisabled();
});

test("mobile workspace reset keeps keyboard focus in its confirmation", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await mockWorkbench(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Open space library" }).click();
  await openSettings(page);
  await expect(page.getByRole("button", { name: "Toggle settings inspector" })).toBeDisabled();
  const reset = page.locator("[data-settings-content]").getByRole("button", { name: "Reset", exact: true });
  await reset.click();
  const confirmation = page.getByRole("dialog", { name: "Reset saved workspace" });
  await expect(confirmation.getByRole("button", { name: "Close", exact: true })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(confirmation.getByRole("button", { name: "Cancel", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(confirmation).toBeHidden();
  await expect(page.getByRole("dialog", { name: "General Inspector" })).toHaveCount(0);
  await expect(reset).toBeFocused();
});

async function expectSeparateEditorControls(header: Locator, name: string) {
  const controls = await header.getByRole("button").evaluateAll((buttons) => buttons.map((button) => {
    const rect = button.getBoundingClientRect();
    return { label: button.getAttribute("aria-label") ?? button.textContent, left: rect.left, right: rect.right, width: rect.width };
  }).filter((button) => button.width > 0));
  const bounds = await header.boundingBox();
  for (const [index, control] of controls.entries()) {
    expect(control.left, control.label ?? "control").toBeGreaterThanOrEqual(bounds!.x);
    expect(control.right, control.label ?? "control").toBeLessThanOrEqual(bounds!.x + bounds!.width);
    if (index > 0) expect(control.left, `${controls[index - 1].label} / ${control.label}`).toBeGreaterThanOrEqual(controls[index - 1].right);
  }
  expect((await header.getByText(name, { exact: true }).boundingBox())?.width).toBeGreaterThan(0);
}

for (const profile of [
  { name: "desktop", width: 1440, hasTouch: false },
  { name: "mobile", width: 390, hasTouch: true },
  { name: "touch tablet", width: 900, hasTouch: true }
]) {
  test.describe(profile.name, () => {
    test.use({ viewport: { width: profile.width, height: 900 }, hasTouch: profile.hasTouch });

    test("keeps controls usable at the intended pointer density", async ({ page }) => {
      await mockWorkbench(page);
      await page.goto("/");
      await page.getByRole("button", { name: "Open space library" }).click();
      if (profile.width < 768) await page.getByRole("button", { name: "Inspect Daily" }).click();

      const toggle = page.getByRole("switch", { name: "Pin to navigation" });
      const toggleBox = await toggle.boundingBox();
      expect(toggleBox?.height).toBe(profile.hasTouch ? 44 : 24);
      expect(toggleBox?.width).toBe(profile.hasTouch ? 44 : 40);
      await expect(toggle).toHaveAttribute("aria-checked", "true");
      await toggle.click({ position: { x: 2, y: 2 } });
      await expect(toggle).toHaveAttribute("aria-checked", "false");
      if (profile.width < 768) await page.getByRole("dialog", { name: "Space Inspector" }).getByRole("button", { name: "Close", exact: true }).click();

      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.locator("[data-settings-view]");
      const tab = settings.getByRole("tab", { name: "Agents" });
      expect((await tab.boundingBox())?.height).toBe(profile.hasTouch ? 44 : 32);
      await tab.click();
      await expect(page.getByRole("button", { name: "Toggle settings inspector" })).toBeDisabled();
      await expect(settings.getByRole("button", { name: "Add agent", exact: true })).toHaveCSS("height", "48px");
      await settings.getByRole("button", { name: "Add agent", exact: true }).click();
      const field = settings.getByRole("textbox", { name: "New agent name" });
      expect((await field.boundingBox())?.height).toBe(profile.hasTouch ? 44 : 28);
      await expect(field).toBeFocused();
      await page.keyboard.press("Tab");
      await page.keyboard.press("Shift+Tab");
      await expect(field).toBeFocused();
      const focus = await field.evaluate((element) => ({ visible: element.matches(":focus-visible"), shadow: getComputedStyle(element).boxShadow }));
      expect(focus.visible).toBe(true);
      expect(focus.shadow).not.toBe("none");
    });

    if (profile.name === "touch tablet") {
      for (const name of ["notes.md", "data.json"]) {
        test(`keeps narrow ${name} editor controls separate and secondary actions accessible`, async ({ page }) => {
          const node = await mockWorkbench(page, 1, { name, path: `/${name}` });
          await page.goto("/");
          await page.getByRole("button", { name: node.name, exact: true }).click();
          const header = page.locator("[data-editor-group-header]");
          await expect(header.getByRole("button", { name: "Version history", exact: true, includeHidden: true })).toBeAttached();
          for (const [viewportWidth, editorWidth] of [[900, 320], [1164, 500], [1324, 660]]) {
            await page.setViewportSize({ width: viewportWidth, height: 900 });
            await expect(header).toHaveCSS("width", `${editorWidth}px`);
            await expectSeparateEditorControls(header, node.name);
            await header.getByRole("button", { name: "More actions" }).click();
            const menu = page.getByRole("menu", { name: "Editor actions" });
            for (const action of ["Copy path", "Copy content", "Version history", ...(name.endsWith(".json") ? ["Expand all", "Collapse all"] : [])]) {
              await expect(menu.getByRole("menuitem", { name: action, exact: true })).toBeVisible();
            }
            if (name.endsWith(".json")) {
              await menu.getByRole("menuitem", { name: "Source view", exact: true }).click();
              await expect(page.locator("[data-editor-group] pre")).toHaveText('{"title":"Example"}');
              await header.getByRole("button", { name: "More actions" }).click();
              await menu.getByRole("menuitem", { name: "Tree view", exact: true }).click();
              await expect(page.getByLabel("Structured data tree")).toBeVisible();
            } else {
              await page.keyboard.press("Escape");
            }
            await header.getByRole("button", { name: "Edit", exact: true }).click();
            await expect(header.getByRole("button", { name: "Cancel edit" })).toBeVisible();
            await expectSeparateEditorControls(header, node.name);
            await header.getByRole("button", { name: "Cancel edit" }).click();
            await expect(header.getByRole("button", { name: "Edit", exact: true })).toBeVisible();
          }
          await expect(header.getByRole("button", { name: "Copy content", exact: true })).toBeVisible();
        });
      }

      test("keeps image preview controls separate in a narrow editor", async ({ page }) => {
        const node = await mockWorkbench(page, 1, { name: "image.svg", path: "/image.svg", kind: "file", media_type: "image/svg+xml", preview_available: true });
        await page.goto("/");
        await page.getByRole("button", { name: node.name, exact: true }).click();
        await expect(page.getByRole("img", { name: node.name })).toBeVisible();
        const header = page.locator("[data-editor-group-header]");
        await expect(header).toHaveCSS("width", "320px");
        await expectSeparateEditorControls(header, node.name);
        await expect(header.getByRole("button", { name: "Download", exact: true })).toBeVisible();
        await expect(header.getByRole("button", { name: "Copy path", exact: true })).toBeVisible();
      });

      test("keeps virtual tree rows separate and rail controls within the 44px rail", async ({ page }) => {
        await mockWorkbench(page, 12);
        await page.goto("/");
        const rows = page.locator("[data-tree-index] [data-node-row]");
        await expect(rows).toHaveCount(12);
        const bounds = await rows.evaluateAll((elements) => elements.map((element) => {
          const rect = element.getBoundingClientRect();
          return { top: rect.top, bottom: rect.bottom, height: rect.height };
        }));
        for (const [index, row] of bounds.entries()) {
          expect(row.height).toBe(36);
          if (index > 0) {
            expect(row.top).toBeGreaterThanOrEqual(bounds[index - 1].bottom);
            expect(row.top - bounds[index - 1].top).toBe(38);
          }
        }

        const rail = page.getByRole("complementary", { name: "Space navigation" });
        const railBounds = await rail.boundingBox();
        expect(railBounds?.width).toBe(44);
        const buttons = await rail.getByRole("button").evaluateAll((elements) => elements.map((element) => {
          const rect = element.getBoundingClientRect();
          return { left: rect.left, right: rect.right, width: rect.width };
        }));
        expect(buttons).toHaveLength(4);
        for (const button of buttons) {
          expect(button.width).toBe(44);
          expect(button.left).toBeGreaterThanOrEqual(railBounds!.x);
          expect(button.right).toBeLessThanOrEqual(railBounds!.x + railBounds!.width);
        }
      });
    }
  });
}

test("file and space context menus transfer focus and return it on dismissal", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const node = await mockWorkbench(page);
  await page.goto("/");
  const row = page.getByRole("button", { name: node.name, exact: true });
  await row.focus();
  await row.click({ button: "right" });
  const files = page.getByRole("menu", { name: "File actions" });
  await expect(files.getByRole("menuitem", { name: "Open", exact: true })).toBeFocused();
  await page.keyboard.press("End");
  await expect(files.getByRole("menuitem", { name: "Delete", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(files).toBeHidden();
  await expect(row).toBeFocused();

  const space = page.getByRole("button", { name: "Daily", exact: true });
  await space.focus();
  await space.click({ button: "right" });
  const spaces = page.getByRole("menu", { name: "Space actions" });
  await expect(spaces.getByRole("menuitem", { name: "Select", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(space).toBeFocused();
});
