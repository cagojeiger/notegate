import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ApiKeyListResponse, MintedKey } from "../../api/keys";
import { KeyManager } from "./KeyManager";

const emptyKeys: ApiKeyListResponse = {
  keys: [],
  page: { limit: 100, returned: 0, has_more: false, next_cursor: null }
};

function renderKeyManager({
  maxTtlDays = 30,
  create = vi.fn()
}: {
  maxTtlDays?: number;
  create?: (input: { name: string; expires_at: string }) => Promise<MintedKey>;
} = {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false }
    }
  });
  const list = vi.fn().mockResolvedValue(emptyKeys);
  const revoke = vi.fn().mockResolvedValue(undefined);
  render(
    <QueryClientProvider client={queryClient}>
      <KeyManager
        queryKey={["keys", maxTtlDays]}
        list={list}
        create={create}
        revoke={revoke}
        maxTtlDays={maxTtlDays}
      />
    </QueryClientProvider>
  );
  return { list, revoke };
}

describe("KeyManager", () => {
  it("uses the user-key 30-day cap by default", async () => {
    renderKeyManager();

    await screen.findByText("No active keys.");

    const expires = screen.getByLabelText("Expires");
    expect(within(expires).getByRole("option", { name: "30 days" })).toBeInTheDocument();
    expect(within(expires).queryByRole("option", { name: "365 days" })).not.toBeInTheDocument();
    expect(within(expires).queryByRole("option", { name: "Custom" })).not.toBeInTheDocument();
  });

  it("allows agent keys to use the 365-day preset", async () => {
    const now = Date.parse("2026-06-15T00:00:00.000Z");
    vi.spyOn(Date, "now").mockReturnValue(now);
    const create = vi.fn().mockResolvedValue({
      id: "key-1",
      name: "agent",
      token: "ngk_v1_token",
      expires_at: "2027-06-14T23:55:00.000Z",
      created_at: "2026-06-15T00:00:00.000Z"
    });
    const user = userEvent.setup();
    renderKeyManager({ maxTtlDays: 365, create });

    await screen.findByText("No active keys.");
    await user.type(screen.getByLabelText("Name"), "agent");
    await user.selectOptions(screen.getByLabelText("Expires"), "365");
    await user.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => {
      expect(create.mock.calls[0]?.[0]).toEqual({
        name: "agent",
        expires_at: "2027-06-14T23:55:00.000Z"
      });
    });
  });

  it("does not expose custom expiry input for agent keys", async () => {
    renderKeyManager({ maxTtlDays: 365 });

    await screen.findByText("No active keys.");

    const expires = screen.getByLabelText("Expires");
    expect(within(expires).getByRole("option", { name: "365 days" })).toBeInTheDocument();
    expect(within(expires).queryByRole("option", { name: "Custom" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Custom days/)).not.toBeInTheDocument();
  });
});
