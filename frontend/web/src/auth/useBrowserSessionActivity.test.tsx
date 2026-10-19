import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/errors";
import { useBrowserSessionActivity } from "./useBrowserSessionActivity";

const { post } = vi.hoisted(() => ({ post: vi.fn() }));
const client = { post };
vi.mock("../api/ApiProvider", () => ({ useApiClient: () => client }));

describe("browser session activity", () => {
  afterEach(() => vi.useRealTimers());
  beforeEach(() => {
    post.mockReset().mockResolvedValue(undefined);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  });

  it("records foreground use at most once a minute and never polls", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const { unmount } = renderHook(() => useBrowserSessionActivity(true, vi.fn()));
    await act(async () => {});
    expect(post).toHaveBeenCalledExactlyOnceWith("/auth/session/activity");
    act(() => {
      document.dispatchEvent(new Event("pointerdown"));
      document.dispatchEvent(new Event("keydown"));
      document.dispatchEvent(new Event("wheel"));
    });
    expect(post).toHaveBeenCalledTimes(1);
    act(() => vi.advanceTimersByTime(60 * 60_000));
    expect(post).toHaveBeenCalledTimes(1);
    await act(async () => document.dispatchEvent(new Event("keydown")));
    expect(post).toHaveBeenCalledTimes(2);
    unmount();
    act(() => vi.advanceTimersByTime(60_000));
    act(() => document.dispatchEvent(new Event("pointerdown")));
    expect(post).toHaveBeenCalledTimes(2);
  });

  it("does not renew an unauthenticated or hidden page", async () => {
    const onUnauthorized = vi.fn();
    const { rerender } = renderHook(({ enabled }) => useBrowserSessionActivity(enabled, onUnauthorized), {
      initialProps: { enabled: false }
    });
    act(() => document.dispatchEvent(new Event("keydown")));
    expect(post).not.toHaveBeenCalled();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    rerender({ enabled: true });
    act(() => document.dispatchEvent(new Event("pointerdown")));
    expect(post).not.toHaveBeenCalled();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(post).toHaveBeenCalledOnce();
  });

  it("returns to authentication on 401 while allowing transient failures to retry on use", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_000);
    const onUnauthorized = vi.fn();
    post.mockRejectedValueOnce(new ApiError("unavailable", 503));
    renderHook(() => useBrowserSessionActivity(true, onUnauthorized));
    await act(async () => {});
    expect(onUnauthorized).not.toHaveBeenCalled();
    now.mockReturnValue(61_000);
    post.mockRejectedValueOnce(new ApiError("expired", 401));
    await act(async () => document.dispatchEvent(new Event("pointerdown")));
    expect(onUnauthorized).toHaveBeenCalledOnce();
  });
});
