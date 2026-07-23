import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useDevAuthGateController } from "./useDevAuthGateController";

function dispatchLoginComplete(origin: string, source: MessageEventSource | null = null) {
  window.dispatchEvent(new MessageEvent("message", {
    data: { type: "notegate:login-complete" },
    origin,
    source
  }));
}

describe("useDevAuthGateController", () => {
  it("accepts login completion only from the current origin", async () => {
    const onSessionAuthenticated = vi.fn().mockResolvedValue(true);
    const { result } = renderHook(() => useDevAuthGateController({
      onAuthenticated: vi.fn(),
      onSessionAuthenticated
    }));

    act(() => result.current.beginPolling(null));
    act(() => dispatchLoginComplete("https://attacker.example"));
    expect(onSessionAuthenticated).not.toHaveBeenCalled();

    act(() => dispatchLoginComplete(window.location.origin));
    await waitFor(() => expect(onSessionAuthenticated).toHaveBeenCalledOnce());
  });

  it("rejects same-origin completion messages from a different window", async () => {
    const onSessionAuthenticated = vi.fn().mockResolvedValue(true);
    const popup = { closed: false, focus: vi.fn() } as unknown as Window;
    vi.spyOn(window, "open").mockReturnValue(popup);
    const { result } = renderHook(() => useDevAuthGateController({
      onAuthenticated: vi.fn(),
      onSessionAuthenticated
    }));

    act(() => result.current.startLogin());
    act(() => dispatchLoginComplete(window.location.origin, window));
    expect(onSessionAuthenticated).not.toHaveBeenCalled();

    act(() => dispatchLoginComplete(window.location.origin, popup));
    await waitFor(() => expect(onSessionAuthenticated).toHaveBeenCalledOnce());
  });
});
