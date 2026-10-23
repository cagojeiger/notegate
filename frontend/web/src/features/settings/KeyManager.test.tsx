import { QueryClientProvider, type QueryClient, type QueryKey } from "@tanstack/react-query";
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
  revoke = vi.fn().mockResolvedValue(undefined),
  queryClient = createTestQueryClient(),
  queryKey = ["keys", maxTtlDays]
}: {
  maxTtlDays?: number;
  create?: (input: { name: string; expires_at: string }) => Promise<MintedKey>;
  list?: () => Promise<ApiKeyListResponse>;
  revoke?: (id: string) => Promise<void>;
  queryClient?: QueryClient;
  queryKey?: QueryKey;
} = {}) {
  const view = render(
    <QueryClientProvider client={queryClient}>
      <KeyManager
        queryKey={queryKey}
        list={list}
        create={create}
        revoke={revoke}
        maxTtlDays={maxTtlDays}
      />
    </QueryClientProvider>
  );
  return { ...view, list, revoke, queryClient };
}

describe("KeyManager", () => {
  const mintedKey: MintedKey = {
    id: "key-retained", name: "retained-key", token: "dummy-one-time-token",
    created_at: "2026-10-22T00:00:00Z", expires_at: "2026-11-21T00:00:00Z"
  };

  async function submitKey(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByRole("button", { name: "Add API key" }));
    await user.type(screen.getByLabelText("Name"), "retained-key");
    await user.click(screen.getByRole("button", { name: "Create" }));
  }

  it("retains a pending mint and its one-time result across remounts until Done", async () => {
    let finish!: (key: MintedKey) => void;
    const create = vi.fn(() => new Promise<MintedKey>((resolve) => { finish = resolve; }));
    const queryClient = createTestQueryClient();
    const props = { queryClient, create };
    const user = userEvent.setup();
    const first = renderKeyManager(props);
    await submitKey(user);
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
    first.unmount();

    const second = renderKeyManager(props);
    expect(screen.getByRole("status")).toHaveTextContent("Creating API key");
    expect(screen.getByRole("button", { name: "Add API key" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Add API key" }));
    await act(async () => { finish(mintedKey); });
    expect(await screen.findByText(mintedKey.token)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add API key" })).toBeDisabled();
    expect(create).toHaveBeenCalledTimes(1);
    second.unmount();

    const third = renderKeyManager(props);
    expect(screen.getByText(mintedKey.token)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByText(mintedKey.token)).not.toBeInTheDocument();
    expect(queryClient.getMutationCache().getAll()).toHaveLength(0);
    third.unmount();

    renderKeyManager(props);
    expect(screen.queryByRole("button", { name: "Copy token" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add API key" })).toBeEnabled();
  });

  it("keeps a late mint failure retryable after returning to the view", async () => {
    let fail!: (error: Error) => void;
    const create = vi.fn().mockImplementationOnce(() => new Promise<MintedKey>((_resolve, reject) => { fail = reject; })).mockResolvedValue(mintedKey);
    const queryClient = createTestQueryClient();
    const props = { queryClient, create };
    const user = userEvent.setup();
    const first = renderKeyManager(props);
    await submitKey(user);
    first.unmount();
    await act(async () => { fail(new Error("Agent API key limit reached")); });
    renderKeyManager(props);

    expect(screen.getByRole("alert")).toHaveTextContent("Agent API key limit reached");
    expect(create).toHaveBeenCalledTimes(1);
    await submitKey(user);
    expect(await screen.findByText(mintedKey.token)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(create).toHaveBeenCalledTimes(2);
  });

  it("isolates retained issuance by agent and authenticated query cache", async () => {
    let finish!: (key: MintedKey) => void;
    const create = vi.fn(() => new Promise<MintedKey>((resolve) => { finish = resolve; }));
    const queryClient = createTestQueryClient();
    const queryKey = ["agents", "agent-a", "keys"];
    const user = userEvent.setup();
    const first = renderKeyManager({ queryClient, queryKey, create });
    await submitKey(user);
    first.unmount();

    const otherAgent = renderKeyManager({ queryClient, queryKey: ["agents", "agent-b", "keys"], create });
    expect(screen.queryByText("Creating API key…")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add API key" })).toBeEnabled();
    await act(async () => { finish(mintedKey); });
    expect(screen.queryByText(mintedKey.token)).not.toBeInTheDocument();
    otherAgent.unmount();

    const otherIdentity = renderKeyManager({ queryKey, create });
    expect(screen.queryByText(mintedKey.token)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add API key" })).toBeEnabled();
    otherIdentity.unmount();

    renderKeyManager({ queryClient, queryKey, create });
    expect(screen.getByText(mintedKey.token)).toBeInTheDocument();
    expect(create).toHaveBeenCalledTimes(1);
  });

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
    expect(confirm).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Revoke other-key" })).toBeDisabled();
    await user.click(confirm);
    expect(revoke).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); });
    await waitFor(() => expect(screen.getByRole("button", { name: "Revoke other-key" })).toBeEnabled());
  });

  it("keeps a pending revocation locked across remounts without blocking a different agent", async () => {
    const key = { id: "key-1", name: "ci-key", created_at: "2026-10-01T00:00:00Z", expires_at: "2026-10-31T00:00:00Z" };
    let finish!: () => void;
    const revoke = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const queryClient = createTestQueryClient();
    const props = { queryClient, queryKey: ["agents", "a", "keys"], list: vi.fn().mockResolvedValue({ ...emptyKeys, keys: [key] }), revoke };
    const user = userEvent.setup();
    const first = renderKeyManager(props);
    await user.click(await screen.findByRole("button", { name: "Revoke ci-key" }));
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(revoke).toHaveBeenCalledTimes(1));
    first.unmount();

    const other = renderKeyManager({ ...props, queryKey: ["agents", "b", "keys"] });
    expect(await screen.findByRole("button", { name: "Revoke ci-key" })).toBeEnabled();
    expect(screen.queryByText("Revoking API key…")).not.toBeInTheDocument();
    other.unmount();

    renderKeyManager(props);
    expect(screen.getByRole("status")).toHaveTextContent("Revoking API key…");
    const confirm = screen.getByRole("button", { name: "Confirm" });
    expect(confirm).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await user.click(confirm);
    expect(revoke).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); });
    await waitFor(() => expect(screen.queryByText("Revoking API key…")).not.toBeInTheDocument());
  });

  it("restores a late revocation failure after remount and allows an explicit retry", async () => {
    const key = { id: "key-1", name: "ci-key", created_at: "2026-10-01T00:00:00Z", expires_at: "2026-10-31T00:00:00Z" };
    let fail!: (error: Error) => void;
    const revoke = vi.fn().mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { fail = reject; })).mockResolvedValue(undefined);
    const props = { queryClient: createTestQueryClient(), list: vi.fn().mockResolvedValue({ ...emptyKeys, keys: [key] }), revoke };
    const user = userEvent.setup();
    const first = renderKeyManager(props);
    await user.click(await screen.findByRole("button", { name: "Revoke ci-key" }));
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    first.unmount();
    await act(async () => { fail(new Error("Key service unavailable")); });
    renderKeyManager(props);

    expect(screen.getByRole("alert")).toHaveTextContent("Key service unavailable");
    expect(revoke).toHaveBeenCalledTimes(1);
    await user.click(await screen.findByRole("button", { name: "Revoke ci-key" }));
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(revoke).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
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

  it("keeps key creation retryable and never exposes a token after a failed request", async () => {
    let rejectCreation: (error: Error) => void = () => undefined;
    const create = vi.fn()
      .mockImplementationOnce(() => new Promise<MintedKey>((_resolve, reject) => { rejectCreation = reject; }))
      .mockResolvedValue({ id: "key-1", name: "ci-key", token: "preview-token-not-a-credential", expires_at: "2026-10-31T00:00:00Z", created_at: "2026-10-01T00:00:00Z" });
    const user = userEvent.setup();
    renderKeyManager({ create });
    await screen.findByText("No active keys.");
    await user.click(screen.getByRole("button", { name: "Add API key" }));
    await user.type(screen.getByLabelText("Name"), "ci-key");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create" })).toHaveAttribute("aria-busy", "true");
    await act(async () => { rejectCreation(new Error("Unavailable")); });
    expect(await screen.findByRole("alert")).toHaveTextContent("Unavailable");
    expect(screen.getByLabelText("Name")).toHaveValue("ci-key");
    expect(screen.queryByRole("button", { name: "Copy token" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("button", { name: "Copy token" })).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(create).toHaveBeenCalledTimes(2);
  });

  it("keeps a one-time token visible after copy failure and excludes it from announcements", async () => {
    const token = "preview-token-not-a-credential";
    const create = vi.fn().mockResolvedValue({ id: "key-1", name: "ci-key", token, expires_at: "2026-10-31T00:00:00Z", created_at: "2026-10-01T00:00:00Z" });
    const user = userEvent.setup();
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new Error("Denied")).mockResolvedValue(undefined);
    renderKeyManager({ create });
    await screen.findByText("No active keys.");
    await user.click(screen.getByRole("button", { name: "Add API key" }));
    await user.type(screen.getByLabelText("Name"), "ci-key");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await user.click(await screen.findByRole("button", { name: "Copy token" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not copy token.");
    expect(screen.getByText(token)).toBeInTheDocument();
    expect(screen.getByRole("alert")).not.toHaveTextContent(token);
    expect(screen.getByText(token).closest('[role="status"], [role="alert"], [aria-live]')).toBeNull();
    for (const announcement of screen.getAllByRole("status")) expect(announcement).not.toHaveTextContent(token);
    await user.click(screen.getByRole("button", { name: "Copy token" }));
    expect(await screen.findByText("Token copied.")).toHaveAttribute("role", "status");
    for (const announcement of screen.getAllByRole("status")) expect(announcement).not.toHaveTextContent(token);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByText(token)).not.toBeInTheDocument();
    expect(screen.queryByText("Token copied.")).not.toBeInTheDocument();
  });
});
