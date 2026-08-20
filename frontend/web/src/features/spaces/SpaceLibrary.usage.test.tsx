import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { Space } from "../../api/types";
import type { CurrentUserUsage } from "../../api/usage";
import { makeSpace } from "../../test/fixtures";
import { SpaceLibrary } from "./SpaceLibrary";
import { useUsageQuery } from "./useUsageQueries";

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
    reconciliation: {
      status: "idle",
      availability: { can_trigger: true, reason: null, retry_at: null }
    }
  }]
};

function jsonResponse(body: unknown, status = 200) {
  return Promise.resolve(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

function UsagePollingOwner() {
  useUsageQuery();
  return null;
}

function renderLibrary({ externalPollingOwner = false } = {}) {
  render(
    <ApiProvider authCacheKey="space-library-usage:0">
      {externalPollingOwner ? <UsagePollingOwner /> : null}
      <SpaceLibrary
        spaces={[space]}
        activeSpace={space}
        isMobile={false}
        usagePollingEnabled={!externalPollingOwner}
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

  it("consumes the desktop polling owner's cached usage without a second request", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(() => jsonResponse(usage));

    renderLibrary({ externalPollingOwner: true });

    expect(await screen.findByText("45.9 MB / 128 MB")).toBeInTheDocument();
    expect(fetchMock.mock.calls.filter(([input]) => String(input).endsWith("/me/usage"))).toHaveLength(1);
  });

  it("queues a usage check from the inspector and shows progress", async () => {
    let pending = false;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((_input, init) => {
      if (init?.method === "POST") {
        pending = true;
        return jsonResponse({
          result: "accepted",
          availability: { can_trigger: false, reason: "pending", retry_at: null }
        }, 202);
      }
      return jsonResponse({
        ...usage,
        spaces: usage.spaces.map((item) => ({
          ...item,
          reconciliation: pending
            ? {
              status: "pending",
              availability: { can_trigger: false, reason: "pending", retry_at: null }
            }
            : item.reconciliation
        }))
      });
    });
    const user = userEvent.setup();
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: "Recalculate Personal usage" }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/spaces/space-1/actions/reconcile-usage",
      expect.objectContaining({ method: "POST" })
    ));
    expect(await screen.findByText("Recalculating usage…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Recalculate Personal usage" })).toBeDisabled();
  });

  it("presents a reconciliation cooldown as up to date", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((input, init) => {
      if (init?.method === "POST") {
        return jsonResponse({ kind: "usage_reconciliation_cooldown", message: "space usage was reconciled recently; try again later" }, 409);
      }
      if (String(input).endsWith("/link-index")) return jsonResponse({
        status: "idle",
        availability: { can_trigger: true, reason: null, retry_at: null }
      });
      return jsonResponse({
        ...usage,
        spaces: usage.spaces.map((item) => ({
          ...item,
          reconciliation: {
            status: "idle",
            availability: {
              can_trigger: false,
              reason: "cooldown",
              retry_at: "2099-01-01T00:00:00Z"
            }
          }
        }))
      });
    });
    const user = userEvent.setup();
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: "Recalculate Personal usage" }));

    expect(await screen.findByText("Usage is already up to date.")).toBeInTheDocument();
    expect(screen.queryByText(/reconciled recently/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Recalculate Personal usage" })).toBeDisabled();
  });

  it("retries after the usage query fails", async () => {
    let failures = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation((input) => {
      if (String(input).endsWith("/link-index")) return jsonResponse({
        status: "idle",
        availability: { can_trigger: true, reason: null, retry_at: null }
      });
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

    expect(await screen.findByRole("button", { name: "Recalculate Personal usage" })).toBeInTheDocument();
  });
});
