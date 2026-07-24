import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useViewportWidth } from "./useMediaQuery";

describe("useViewportWidth", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("coalesces resize bursts into the latest animation frame", () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200, writable: true });
    let frame: FrameRequestCallback | null = null;
    const requestFrame = vi.fn((callback: FrameRequestCallback) => {
      frame = callback;
      return 1;
    });
    vi.stubGlobal("requestAnimationFrame", requestFrame);
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    const { result } = renderHook(() => useViewportWidth());

    act(() => {
      window.innerWidth = 1000;
      window.dispatchEvent(new Event("resize"));
      window.innerWidth = 900;
      window.dispatchEvent(new Event("resize"));
    });

    expect(requestFrame).toHaveBeenCalledTimes(1);
    expect(result.current).toBe(1200);
    act(() => frame?.(0));
    expect(result.current).toBe(900);
  });

  it("cancels a pending resize frame on unmount", () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1200, writable: true });
    vi.stubGlobal("requestAnimationFrame", vi.fn().mockReturnValue(7));
    const cancelFrame = vi.fn();
    vi.stubGlobal("cancelAnimationFrame", cancelFrame);
    const { unmount } = renderHook(() => useViewportWidth());

    act(() => window.dispatchEvent(new Event("resize")));
    unmount();

    expect(cancelFrame).toHaveBeenCalledWith(7);
  });
});
