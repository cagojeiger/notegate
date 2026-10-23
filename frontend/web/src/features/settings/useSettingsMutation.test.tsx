import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { useSettingsMutation } from "./useSettingsMutation";

function cacheScope() {
  const client = new QueryClient();
  return { client, wrapper: ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> };
}

function deferred() {
  let resolve!: (value: string) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<string>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("useSettingsMutation", () => {
  it("retains pending work and success across remounts and blocks stale callers synchronously", async () => {
    const scope = cacheScope();
    const response = deferred();
    const request = vi.fn((_input: string) => response.promise);
    const useOperation = () => useSettingsMutation("trash", { mutationFn: request });
    const first = renderHook(useOperation, scope);
    const staleMutate = first.result.current.mutate;
    act(() => { staleMutate("original"); staleMutate("duplicate"); first.result.current.reset(); });
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    first.unmount();
    const second = renderHook(useOperation, scope);
    expect(second.result.current.isPending).toBe(true);
    act(() => { staleMutate("stale"); second.result.current.mutate("new"); second.result.current.reset(); });
    expect(request).toHaveBeenCalledTimes(1);
    act(() => response.resolve("done"));
    await waitFor(() => expect(second.result.current.isSuccess).toBe(true));
    second.unmount();
    const third = renderHook(useOperation, scope);
    expect(third.result.current.data).toBe("done");
    expect(third.result.current.variables).toBe("original");
    act(() => third.result.current.reset());
    await waitFor(() => expect(third.result.current.isSuccess).toBe(false));
    expect(scope.client.getMutationCache().getAll()).toHaveLength(0);
  });

  it("retains a late failure while its view is absent and requires an explicit retry", async () => {
    const scope = cacheScope();
    const response = deferred();
    const request = vi.fn((_input: string) => response.promise);
    const useOperation = () => useSettingsMutation("revoke-browser", { mutationFn: request });
    const first = renderHook(useOperation, scope);
    act(() => first.result.current.mutate("session"));
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    first.unmount();
    const failure = new Error("Unavailable");
    act(() => response.reject(failure));
    await waitFor(() => expect(scope.client.getMutationCache().getAll()[0].state.status).toBe("error"));
    const second = renderHook(useOperation, scope);
    expect(second.result.current.error).toBe(failure);
    expect(second.result.current.isError).toBe(true);
    expect(request).toHaveBeenCalledTimes(1);
    request.mockResolvedValueOnce("revoked");
    act(() => second.result.current.mutate("session"));
    await waitFor(() => expect(second.result.current.data).toBe("revoked"));
    expect(request).toHaveBeenCalledTimes(2);
  });

  it("keeps independent operation families isolated", async () => {
    const scope = cacheScope();
    const trash = deferred();
    const account = deferred();
    const view = renderHook(() => ({
      trash: useSettingsMutation("trash", { mutationFn: (_id: string) => trash.promise }),
      account: useSettingsMutation("revoke-browser", { mutationFn: (_id: string) => account.promise })
    }), scope);
    act(() => { view.result.current.trash.mutate("item"); view.result.current.account.mutate("session"); });
    await waitFor(() => expect(view.result.current.trash.isPending && view.result.current.account.isPending).toBe(true));
    act(() => { trash.resolve("restored"); account.resolve("revoked"); });
    await waitFor(() => expect(view.result.current.trash.isSuccess && view.result.current.account.isSuccess).toBe(true));
    act(() => view.result.current.trash.reset());
    await waitFor(() => expect(view.result.current.trash.isSuccess).toBe(false));
    expect(view.result.current.account.data).toBe("revoked");
  });

  it("does not expose another authenticated query cache's pending operation", async () => {
    const response = deferred();
    const useOperation = () => useSettingsMutation("trash", { mutationFn: (_id: string) => response.promise });
    const first = renderHook(useOperation, cacheScope());
    act(() => first.result.current.mutate("private-item"));
    await waitFor(() => expect(first.result.current.isPending).toBe(true));
    const second = renderHook(useOperation, cacheScope());
    expect(second.result.current.isPending).toBe(false);
    expect(second.result.current.variables).toBeUndefined();
    act(() => response.resolve("done"));
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    expect(second.result.current.data).toBeUndefined();
  });
});
