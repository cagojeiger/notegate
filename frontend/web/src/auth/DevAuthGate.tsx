import { FormEvent, useEffect, useRef, useState } from "react";

import { writeDevApiKey } from "./session";

type DevAuthGateProps = {
  onAuthenticated: (apiKey: string) => void;
  onSessionAuthenticated: () => Promise<boolean>;
};

function loginUrl(): string {
  return "/auth/login";
}

export function DevAuthGate({ onAuthenticated, onSessionAuthenticated }: DevAuthGateProps) {
  const [apiKey, setApiKey] = useState("");
  const [loginHint, setLoginHint] = useState<string | null>(null);
  const popupCheckRef = useRef<number | null>(null);

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

  async function checkSession(): Promise<boolean> {
    const isAuthenticated = await onSessionAuthenticated();
    if (isAuthenticated && popupCheckRef.current !== null) {
      window.clearInterval(popupCheckRef.current);
      popupCheckRef.current = null;
    }
    return isAuthenticated;
  }

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

  return (
    <main className="grid h-full place-items-center bg-bg px-6 text-text">
      <form onSubmit={handleSubmit} className="w-full max-w-md rounded-2xl border border-border bg-panel p-6 shadow-2xl shadow-black/20">
        <div className="mb-6 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-primary text-lg font-bold text-bg">N</div>
          <div>
            <h1 className="text-xl font-semibold">Sign in to Notegate</h1>
            <p className="text-sm text-muted">Use your Notegate account to open the dashboard.</p>
          </div>
        </div>
        <button
          type="button"
          className="block w-full rounded-lg bg-primary px-4 py-2 text-center text-sm font-semibold text-primary-contrast shadow-[var(--ng-inset-shadow)] transition hover:bg-[var(--ng-primary-hover)]"
          onClick={startLogin}
        >
          Continue with login
        </button>
        <a
          href={loginUrl()}
          target="notegate-login"
          onClick={() => beginPolling(null)}
          className="mt-2 block text-center text-xs text-muted underline underline-offset-2 hover:text-text"
        >
          Or open the login page in a new window
        </a>
        <p className="mt-3 text-xs leading-5 text-muted">OAuth creates an HttpOnly browser session cookie.</p>
        {loginHint ? <p className="mt-3 rounded-lg border border-border bg-surface px-3 py-2 text-xs leading-5 text-muted">{loginHint}</p> : null}
        <details className="mt-5 rounded-xl border border-border bg-surface p-3">
          <summary className="cursor-pointer text-sm font-medium">Developer API key fallback</summary>
          <label className="mt-4 block text-sm font-medium" htmlFor="api-key">
            User API key
          </label>
          <input
            id="api-key"
            name="apiKey"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            className="mt-2 w-full rounded-lg border border-border bg-bg px-3 py-2 font-mono text-sm outline-none ring-primary/0 transition focus:border-primary focus:ring-2 focus:ring-primary/30"
            placeholder="ng_user_..."
            autoComplete="off"
          />
          <button
            type="submit"
            className="mt-4 w-full rounded-lg border border-border bg-panel px-4 py-2 text-sm font-semibold transition hover:bg-panel-strong disabled:cursor-not-allowed disabled:opacity-50"
            disabled={!apiKey.trim()}
          >
            Open with API key
          </button>
        </details>
      </form>
    </main>
  );
}
