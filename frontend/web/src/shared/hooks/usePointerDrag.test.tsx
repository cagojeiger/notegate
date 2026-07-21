import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { usePointerDrag } from "./usePointerDrag";

describe("usePointerDrag", () => {
  afterEach(() => document.body.classList.remove("select-none"));

  it("tracks pointer movement and cleans up when the gesture ends", () => {
    const onMove = vi.fn();
    const { result } = renderHook(() => usePointerDrag());

    act(() => result.current(onMove));
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 120 }));
    expect(onMove).toHaveBeenCalledOnce();
    expect(document.body).toHaveClass("select-none");

    window.dispatchEvent(new PointerEvent("pointerup"));
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 240 }));
    expect(onMove).toHaveBeenCalledOnce();
    expect(document.body).not.toHaveClass("select-none");
  });

  it("cleans up an active gesture when its consumer unmounts", () => {
    const onMove = vi.fn();
    const { result, unmount } = renderHook(() => usePointerDrag());

    act(() => result.current(onMove));
    unmount();
    window.dispatchEvent(new PointerEvent("pointermove"));

    expect(onMove).not.toHaveBeenCalled();
    expect(document.body).not.toHaveClass("select-none");
  });
});
