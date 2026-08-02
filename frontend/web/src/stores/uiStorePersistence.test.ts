import { afterEach, describe, expect, it, vi } from "vitest";

import { browserUiStorePersistence } from "./uiStorePersistence";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("browserUiStorePersistence", () => {
  it("prefers a stored theme over the system preference", () => {
    window.localStorage.setItem("notegate.theme", "light");
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true })));

    expect(browserUiStorePersistence.loadTheme()).toBe("light");
  });

  it("falls back to the system preference when the stored theme is invalid", () => {
    window.localStorage.setItem("notegate.theme", "sepia");
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true })));

    expect(browserUiStorePersistence.loadTheme()).toBe("dark");
  });

  it("applies the initial theme without saving it", () => {
    browserUiStorePersistence.applyTheme("dark");

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem("notegate.theme")).toBeNull();
  });

  it("persists theme and last active space using the existing keys", () => {
    browserUiStorePersistence.saveTheme("dark");
    browserUiStorePersistence.saveLastActiveSpaceId("space-2");

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem("notegate.theme")).toBe("dark");
    expect(window.localStorage.getItem("notegate.lastActiveSpaceId")).toBe("space-2");
  });

  it("falls back when browser storage reads are unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false })));

    expect(browserUiStorePersistence.loadTheme()).toBe("light");
    expect(browserUiStorePersistence.loadLastActiveSpaceId()).toBeNull();
  });

  it("applies the theme when browser storage writes are unavailable", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });

    expect(() => browserUiStorePersistence.saveTheme("dark")).not.toThrow();
    expect(() => browserUiStorePersistence.saveLastActiveSpaceId("space-2")).not.toThrow();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
