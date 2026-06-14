import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { isPageVisible, usePageVisible } from "./usePageVisible";

function setVisibility(value: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", { configurable: true, value });
  act(() => document.dispatchEvent(new Event("visibilitychange")));
}

describe("usePageVisible", () => {
  afterEach(() => setVisibility("visible"));

  it("tracks document visibility", () => {
    setVisibility("visible");
    const { result } = renderHook(() => usePageVisible());

    expect(result.current).toBe(true);

    setVisibility("hidden");
    expect(result.current).toBe(false);

    setVisibility("visible");
    expect(result.current).toBe(true);
  });

  it("reports the current visibility outside React", () => {
    setVisibility("hidden");
    expect(isPageVisible()).toBe(false);
  });
});
