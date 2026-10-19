import { useEffect } from "react";

import { useApiClient } from "../api/ApiProvider";
import { ApiError } from "../api/errors";

export function useBrowserSessionActivity(enabled: boolean, onUnauthorized: () => void) {
  const client = useApiClient();

  useEffect(() => {
    if (!enabled) return;
    let nextAttempt = 0;
    let disposed = false;
    const recordActivity = () => {
      if (document.visibilityState !== "visible" || Date.now() < nextAttempt) return;
      nextAttempt = Date.now() + 60_000;
      void client.post("/auth/session/activity").catch((error: unknown) => {
        if (!disposed && error instanceof ApiError && error.status === 401) onUnauthorized();
      });
    };
    const events = ["pointerdown", "keydown", "wheel", "visibilitychange"];
    for (const event of events) document.addEventListener(event, recordActivity, { passive: true, capture: true });
    recordActivity();
    return () => {
      disposed = true;
      for (const event of events) document.removeEventListener(event, recordActivity, true);
    };
  }, [client, enabled, onUnauthorized]);
}
