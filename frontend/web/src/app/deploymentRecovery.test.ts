import { describe, expect, it, vi } from "vitest";

import { DEPLOYMENT_RELOAD_PROMPT, handleDeploymentPreloadError } from "./deploymentRecovery";

describe("deployment recovery", () => {
  it("reloads after a stale dynamic import when the user confirms", () => {
    const event = new Event("vite:preloadError", { cancelable: true });
    const confirmReload = vi.fn(() => true);
    const reload = vi.fn();

    handleDeploymentPreloadError(event, confirmReload, reload);

    expect(event.defaultPrevented).toBe(true);
    expect(confirmReload).toHaveBeenCalledWith(DEPLOYMENT_RELOAD_PROMPT);
    expect(reload).toHaveBeenCalledOnce();
  });

  it("does not reload when the user cancels", () => {
    const event = new Event("vite:preloadError", { cancelable: true });
    const reload = vi.fn();

    handleDeploymentPreloadError(event, () => false, reload);

    expect(event.defaultPrevented).toBe(true);
    expect(reload).not.toHaveBeenCalled();
  });
});
