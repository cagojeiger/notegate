import { useCallback, useEffect, useRef, useState } from "react";

import type { LoginFlow } from "./loginFlow";

export type LoginGateControllerProps = {
  loginFlow?: LoginFlow;
  onSessionAuthenticated: () => Promise<boolean>;
};

export function loginUrl(): string {
  return "/auth/login";
}

export function useLoginGateController({ loginFlow, onSessionAuthenticated }: LoginGateControllerProps) {
  const [loginHint, setLoginHint] = useState<string | null>(null);
  const popupCheckRef = useRef<number | null>(null);
  const loginPopupRef = useRef<Window | null>(null);

  const checkSession = useCallback(async (): Promise<boolean> => {
    const isAuthenticated = await onSessionAuthenticated();
    if (isAuthenticated && popupCheckRef.current !== null) {
      window.clearInterval(popupCheckRef.current);
      popupCheckRef.current = null;
      loginPopupRef.current = null;
    }
    return isAuthenticated;
  }, [onSessionAuthenticated]);

  useEffect(() => {
    function handleMessage(event: MessageEvent) {
      if (popupCheckRef.current === null) return;
      if (event.origin !== window.location.origin) return;
      if (loginPopupRef.current && event.source !== loginPopupRef.current) return;
      if ((event.data as { type?: string } | null)?.type !== "notegate:login-complete") return;
      void checkSession();
    }
    window.addEventListener("message", handleMessage);
    return () => {
      window.removeEventListener("message", handleMessage);
      if (popupCheckRef.current !== null) window.clearInterval(popupCheckRef.current);
      loginPopupRef.current = null;
    };
  }, [checkSession]);

  function beginPolling(popup: Window | null) {
    if (popupCheckRef.current !== null) window.clearInterval(popupCheckRef.current);
    loginPopupRef.current = popup;
    popupCheckRef.current = window.setInterval(() => {
      void checkSession();
      try {
        if (popup && popup.closed) {
          if (popupCheckRef.current !== null) window.clearInterval(popupCheckRef.current);
          popupCheckRef.current = null;
          loginPopupRef.current = null;
          void checkSession();
        }
      } catch {
        // Some identity providers isolate popups with COOP. Session polling above is
        // the reliable completion signal in that case.
      }
    }, 700);
  }

  async function startLogin() {
    if (loginFlow) {
      setLoginHint("Complete login in the system authentication window.");
      try {
        await loginFlow.start();
        const authenticated = await checkSession();
        if (!authenticated) setLoginHint("Login completed, but NoteGate could not verify the session.");
      } catch {
        setLoginHint("Login was not completed. Try again.");
      }
      return;
    }

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
    loginHint,
    loginHref: loginFlow ? null : loginUrl(),
    startLogin,
    beginPolling
  };
}
