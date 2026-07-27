import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { Space } from "../../api/types";
import type { CurrentUserUsage } from "../../api/usage";
import { makeSpace } from "../../test/fixtures";
import { SpaceLibrary } from "./SpaceLibrary";

vi.mock("./useSpaceQueries", () => ({
  useReorderSpacesMutation: () => ({ isPending: false, mutate: vi.fn() }),
  useUpdateSpaceMutation: () => ({ isError: false, isPending: false, mutate: vi.fn() })
}));

const space: Space = makeSpace({
  id: "space-1",
  name: "Personal",
  sort_order: 1000,
  root_node_id: "root-1",
  updated_at: "2026-07-25T00:00:00Z"
});

const usage: CurrentUserUsage = {
  tier: "tier0",
  spaces: [{
    id: space.id,
    name: space.name,
    items: { used: 319, limit: 1_999 },
    text_bytes: { used: 48_120_320, limit: 134_217_728 },
    file_bytes: { used: 80_000_000, limit: 134_217_728 },
    reconciliation_pending: false
  }]
};

function jsonResponse(body: unknown, status = 200) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

function renderLibrary() {
  render(
    <ApiProvider apiKey="test-key" authCacheKey="space-library-usage:0">
      <SpaceLibrary
        spaces={[space]}
        activeSpace={space}
        isMobile={false}
        inspectorOpen
        onOpenInspector={vi.fn()}
        onCloseInspector={vi.fn()}
        onOpenSpace={vi.fn()}
        onCreateSpace={vi.fn()}
      />
    </ApiProvider>
  );
}

describe("SpaceLibrary usage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("shows independent usage limits in the inspector", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => jsonResponse(usage));

    renderLibrary();

    expect(await screen.findByText("45.9 MB / 128 MB")).toBeInTheDocument();
    expect(screen.getByText("76.3 MB / 128 MB")).toBeInTheDocument();
    expect(screen.getByText("319 / 1,999")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Text usage" })).toHaveAttribute("aria-valuenow", "48120320");
    expect(screen.getByRole("progressbar", { name: "Files usage" })).toHaveAttribute("aria-valuenow", "80000000");
    expect(screen.getByRole("progressbar", { name: "Items usage" })).toHaveAttribute("aria-valuenow", "319");
  });

  it("queues a usage check from the inspector and shows progress", async () => {
    let pending = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => {
      if (init?.method === "POST") {
        pending = true;
        return jsonResponse({ status: "queued" }, 202);
      }
      return jsonResponse({
        ...usage,
        spaces: usage.spaces.map((item) => ({ ...item, reconciliation_pending: pending }))
      });
    });
    const user = userEvent.setup();
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/usage/reconcile",
      expect.objectContaining({ method: "POST" })
    ));
    expect(await screen.findByText("Checking usage…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check Personal usage" })).toBeDisabled();
  });

  it("presents a reconciliation cooldown as up to date", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => init?.method === "POST"
      ? jsonResponse({ kind: "usage_reconciliation_cooldown", message: "space usage was reconciled recently; try again later" }, 409)
      : jsonResponse(usage));
    const user = userEvent.setup();
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: "Check Personal usage" }));

    expect(await screen.findByText("Usage is already up to date.")).toBeInTheDocument();
    expect(screen.queryByText(/reconciled recently/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check Personal usage" })).toBeEnabled();
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
    renderLibrary();

    const retry = await screen.findByRole("button", { name: "Retry Personal usage" }, { timeout: 3_000 });
    await user.click(retry);

    expect(await screen.findByRole("button", { name: "Check Personal usage" })).toBeInTheDocument();
  });
});
