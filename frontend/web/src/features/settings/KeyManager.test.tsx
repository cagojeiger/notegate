import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ApiKeyListResponse, MintedKey } from "../../api/keys";
import { createTestQueryClient } from "../../test/queryClient";
import { KeyManager } from "./KeyManager";

const emptyKeys: ApiKeyListResponse = {
  keys: [],
  page: { limit: 100, returned: 0, has_more: false, next_cursor: null }
};

function renderKeyManager({
  maxTtlDays = 30,
  create = vi.fn(),
  list = vi.fn().mockResolvedValue(emptyKeys),
  revoke = vi.fn().mockResolvedValue(undefined)
}: {
  maxTtlDays?: number;
  create?: (input: { name: string; expires_at: string }) => Promise<MintedKey>;
  list?: () => Promise<ApiKeyListResponse>;
  revoke?: (id: string) => Promise<void>;
} = {}) {
  const queryClient = createTestQueryClient();
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
  return { list, revoke, queryClient };
}

describe("KeyManager", () => {
  it("prevents repeated revocation and target changes while a revocation is pending", async () => {
    const keys = ["ci-key", "other-key"].map((name, index) => ({ id: `key-${index}`, name, created_at: "2026-10-01T00:00:00Z", expires_at: "2026-10-31T00:00:00Z" }));
    let finish!: () => void;
    const revoke = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    renderKeyManager({ list: vi.fn().mockResolvedValue({ ...emptyKeys, keys }), revoke });
    await user.click(await screen.findByRole("button", { name: "Revoke ci-key" }));
    const confirm = screen.getByRole("button", { name: "Confirm" });
    await user.click(confirm);

    expect(confirm).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Revoke other-key" })).toBeDisabled();
    await user.click(confirm);
    expect(revoke).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); });
    await waitFor(() => expect(screen.getByRole("button", { name: "Revoke other-key" })).toBeEnabled());
  });

  it("distinguishes a failed read from an empty key list and retries it", async () => {
    const list = vi.fn().mockRejectedValueOnce(new Error("Unavailable")).mockResolvedValue(emptyKeys);
    const user = userEvent.setup();
    renderKeyManager({ list });

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load API keys.");
    expect(screen.queryByText("No active keys.")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry API keys" }));

    expect(await screen.findByText("No active keys.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("does not present cached keys as current after a refresh fails", async () => {
    const key = { id: "key-1", name: "ci-key", created_at: "2026-10-01T00:00:00Z", expires_at: "2026-10-31T00:00:00Z" };
    const list = vi.fn().mockResolvedValueOnce({ ...emptyKeys, keys: [key] }).mockRejectedValue(new Error("Unavailable"));
    const { queryClient } = renderKeyManager({ list });
    await screen.findByRole("button", { name: "Revoke ci-key" });

    await act(async () => { await queryClient.invalidateQueries({ queryKey: ["keys", 30] }); });

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load API keys.");
    expect(screen.queryByRole("button", { name: "Revoke ci-key" })).not.toBeInTheDocument();
    expect(screen.queryByText("No active keys.")).not.toBeInTheDocument();
  });

  it("uses the user-key 30-day cap by default", async () => {
    const user = userEvent.setup();
    renderKeyManager();

    await screen.findByText("No active keys.");
    await user.click(screen.getByRole("button", { name: "Add API key" }));

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
      token: "ngk_v2_token",
      expires_at: "2027-06-14T23:55:00.000Z",
      created_at: "2026-06-15T00:00:00.000Z"
    });
    const user = userEvent.setup();
    renderKeyManager({ maxTtlDays: 365, create });

    await screen.findByText("No active keys.");
    await user.click(screen.getByRole("button", { name: "Add API key" }));
    await user.type(screen.getByLabelText("Name"), "agent");
    await user.selectOptions(screen.getByLabelText("Expires"), "365");
    await user.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => {
      expect(create.mock.calls[0]?.[0]).toEqual({
        name: "agent",
        expires_at: "2027-06-14T23:55:00.000Z"
      });
    });
    expect(screen.getByRole("button", { name: "Copy token" })).toHaveFocus();
    expect(screen.getByText("ngk_v2_token")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByText("ngk_v2_token")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Add API key" })).toHaveFocus());
  });

  it("does not expose custom expiry input for agent keys", async () => {
    const user = userEvent.setup();
    renderKeyManager({ maxTtlDays: 365 });

    await screen.findByText("No active keys.");
    await user.click(screen.getByRole("button", { name: "Add API key" }));

    const expires = screen.getByLabelText("Expires");
    expect(within(expires).getByRole("option", { name: "365 days" })).toBeInTheDocument();
    expect(within(expires).queryByRole("option", { name: "Custom" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Custom days/)).not.toBeInTheDocument();
  });

  it("opens the creation form from the list footer and restores focus after cancellation", async () => {
    const create = vi.fn();
    const user = userEvent.setup();
    renderKeyManager({ create });
    expect(screen.queryByLabelText("Name")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add API key" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.type(screen.getByLabelText("Name"), "unfinished");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByLabelText("Name")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Add API key" })).toHaveFocus());
    expect(create).not.toHaveBeenCalled();
  });
});
