import { FormEvent, useEffect, useRef, useState } from "react";

import { writeDevApiKey } from "./session";

export type DevAuthGateControllerProps = {
  onAuthenticated: (apiKey: string) => void;
  onSessionAuthenticated: () => Promise<boolean>;
};

export function loginUrl(): string {
  return "/auth/login";
}

export function useDevAuthGateController({ onAuthenticated, onSessionAuthenticated }: DevAuthGateControllerProps) {
  const [apiKey, setApiKey] = useState("");
  const [loginHint, setLoginHint] = useState<string | null>(null);
  const popupCheckRef = useRef<number | null>(null);

  async function checkSession(): Promise<boolean> {
    const isAuthenticated = await onSessionAuthenticated();
    if (isAuthenticated && popupCheckRef.current !== null) {
      window.clearInterval(popupCheckRef.current);
      popupCheckRef.current = null;
    }
    return isAuthenticated;
  }

  useEffect(() => {
    function handleMessage(event: MessageEvent) {
      if ((event.data as { type?: string } | null)?.type === "notegate:login-complete") {
        void checkSession();
      }
    }
    window.addEventListener("message", handleMessage);
    return () => {
      window.removeEventListener("message", handleMessage);
      if (popupCheckRef.current !== null) window.clearInterval(popupCheckRef.current);
    };
  }, [onSessionAuthenticated]);

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = apiKey.trim();
    if (!trimmed) return;
    writeDevApiKey(trimmed);
    onAuthenticated(trimmed);
  }

  function beginPolling(popup: Window | null) {
    if (popupCheckRef.current !== null) window.clearInterval(popupCheckRef.current);
    popupCheckRef.current = window.setInterval(() => {
      void checkSession();
      try {
        if (popup && popup.closed) {
          if (popupCheckRef.current !== null) window.clearInterval(popupCheckRef.current);
          popupCheckRef.current = null;
          void checkSession();
        }
      } catch {
        // Some identity providers isolate popups with COOP. Session polling above is
        // the reliable completion signal in that case.
      }
    }, 700);
  }

  function startLogin() {
    // Open straight to the login URL in the click handler. Opening a blank window
    // first and redirecting it is what aggressive popup blockers target most, so a
    // direct navigation is the most blocker-tolerant form.
    const popup = window.open(loginUrl(), "notegate-login", "popup,width=520,height=720");
    if (!popup) {
      setLoginHint("Popup was blocked. Use the “Open login page” link below, or allow popups for this site.");
      beginPolling(null);
      return;
    }
    setLoginHint("Complete login in the popup. This page will continue automatically.");
    beginPolling(popup);
    popup.focus();
  }

  return {
    apiKey,
    loginHint,
    loginHref: loginUrl(),
    canSubmitApiKey: Boolean(apiKey.trim()),
    setApiKey,
    handleSubmit,
    startLogin,
    beginPolling
  };
}
