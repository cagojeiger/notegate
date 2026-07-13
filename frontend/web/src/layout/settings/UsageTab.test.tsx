import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { CurrentUserUsage } from "../../api/usage";
import { UsageTab } from "./UsageTab";

const usage: CurrentUserUsage = {
  tier: "tier0",
  spaces: [{
    id: "space-1",
    name: "Personal",
    items: { used: 319, limit: 1_999 },
    text_bytes: { used: 48_120_320, limit: 134_217_728 },
    file_bytes: { used: 80_000_000, limit: 134_217_728 },
    reconciliation_pending: false
  }]
};

function jsonResponse(body: unknown, status = 200) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

function renderUsage() {
  render(
    <ApiProvider apiKey="test-key" authCacheKey="usage-test:0">
      <UsageTab />
    </ApiProvider>
  );
}

describe("UsageTab", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("shows independent text, file, and visible item limits", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => jsonResponse(usage));

    renderUsage();

    expect(await screen.findByText("Tier 0")).toBeInTheDocument();
    expect(screen.getByText("Space")).toBeInTheDocument();
    expect(screen.getByText("Personal")).toBeInTheDocument();
    expect(screen.getByText("45.9 MB / 128 MB")).toBeInTheDocument();
    expect(screen.getByText("76.3 MB / 128 MB")).toBeInTheDocument();
    expect(screen.getByText("319 / 1,999")).toBeInTheDocument();
    expect(screen.queryByText("API keys")).not.toBeInTheDocument();
    expect(screen.queryByText("Connections")).not.toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Text storage usage" })).toHaveAttribute("aria-valuenow", "48120320");
    expect(screen.getByRole("progressbar", { name: "File storage usage" })).toHaveAttribute("aria-valuenow", "80000000");
    expect(screen.getByRole("progressbar", { name: "Items usage" })).toHaveAttribute("aria-valuenow", "319");
    expect(screen.getByRole("button", { name: "About Text storage" })).toHaveAccessibleDescription("Text content currently stored in this space. Deleted items and metadata are excluded.");
    expect(screen.getByRole("button", { name: "About File storage" })).toHaveAccessibleDescription("File content currently stored in this space. Deleted items are excluded.");
    expect(screen.getByRole("button", { name: "About Items" })).toHaveAccessibleDescription("Folders, Text, and Files currently in this space.");
  });

  it("shows an in-progress state after requesting a usage check", async () => {
    let pending = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((input, init) => {
      if (init?.method === "POST") {
        pending = true;
        return jsonResponse({ status: "queued" }, 202);
      }
      return jsonResponse({
        ...usage,
        spaces: usage.spaces.map((space) => ({
          ...space,
          reconciliation_pending: pending
        }))
      });
    });
    const user = userEvent.setup();
    renderUsage();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/usage/reconcile",
      expect.objectContaining({ method: "POST" })
    ));
    expect(await screen.findByText("Checking…")).toBeInTheDocument();
    expect(screen.getByText("Checking usage…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check Personal usage" })).toBeDisabled();
  });

  it("prevents overlapping usage-check requests across spaces", async () => {
    let resolvePost: ((response: Response) => void) | undefined;
    const postResponse = new Promise<Response>((resolve) => {
      resolvePost = resolve;
    });
    const twoSpaces = {
      ...usage,
      spaces: [usage.spaces[0], { ...usage.spaces[0], id: "space-2", name: "Archive" }]
    };
    vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => init?.method === "POST"
      ? postResponse
      : jsonResponse(twoSpaces));
    const user = userEvent.setup();
    renderUsage();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    expect(screen.getByRole("button", { name: "Check Personal usage" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Check Archive usage" })).toBeDisabled();
    resolvePost?.(new Response(JSON.stringify({ status: "queued" }), {
      status: 202,
      headers: { "content-type": "application/json" }
    }));
  });

  it("keeps a cooldown response user-facing and non-technical", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => init?.method === "POST"
      ? jsonResponse({ kind: "usage_reconciliation_cooldown", message: "space usage was reconciled recently; try again later" }, 409)
      : jsonResponse(usage));
    const user = userEvent.setup();
    renderUsage();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    expect(await screen.findByText("Usage is already up to date.")).toBeInTheDocument();
    expect(screen.queryByText(/reconciled recently/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check Personal usage" })).toBeEnabled();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("refreshes a stale pending state when reconciliation is already queued", async () => {
    let pending = false;
    vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => {
      if (init?.method === "POST") {
        pending = true;
        return jsonResponse({ kind: "usage_reconciliation_pending", message: "space usage reconciliation is already queued" }, 409);
      }
      return jsonResponse({
        ...usage,
        spaces: usage.spaces.map((space) => ({ ...space, reconciliation_pending: pending }))
      });
    });
    const user = userEvent.setup();
    renderUsage();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    expect(await screen.findByText("Checking usage…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check Personal usage" })).toBeDisabled();
    expect(screen.queryByText("Usage is already up to date.")).not.toBeInTheDocument();
  });

  it("clears an already-queued response after the server reports completion", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => init?.method === "POST"
      ? jsonResponse({ kind: "usage_reconciliation_pending", message: "space usage reconciliation is already queued" }, 409)
      : jsonResponse(usage));
    const user = userEvent.setup();
    renderUsage();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    expect(await screen.findByText("Usage is up to date.")).toBeInTheDocument();
    expect(screen.queryByText("Usage could not be checked. Try again shortly.")).not.toBeInTheDocument();
  });

  it("retries after the usage query fails", async () => {
    let failures = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(() => {
      if (failures < 2) {
        failures += 1;
        return jsonResponse({ kind: "internal_error", message: "temporarily unavailable" }, 500);
      }
      return jsonResponse(usage);
    });
    const user = userEvent.setup();
    renderUsage();

    expect(await screen.findByText("Usage is unavailable right now.", {}, { timeout: 3_000 })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));

    expect(await screen.findByText("Personal")).toBeInTheDocument();
  });
});
