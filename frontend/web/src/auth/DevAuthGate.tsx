import { Moon, ShieldCheck, Sun } from "lucide-react";

import type { ThemeMode } from "../design/tokens";
import { BrandLockup, Button, Card, IconButton, TextField } from "../shared/ui";
import { useDevAuthGateController, type DevAuthGateControllerProps } from "./useDevAuthGateController";

type DevAuthGateProps = DevAuthGateControllerProps & {
  devApiKeyFallbackEnabled: boolean;
  theme: ThemeMode;
  onToggleTheme: () => void;
};

export function DevAuthGate({ devApiKeyFallbackEnabled, theme, onToggleTheme, ...controllerProps }: DevAuthGateProps) {
  const auth = useDevAuthGateController(controllerProps);

  return (
    <main className="ng-auth-screen grid h-full overflow-y-auto px-6 py-10 text-text">
      <AuthBackdropMark />

      <div className="absolute right-4 top-4 z-10">
        <IconButton label={`Use ${theme === "light" ? "dark" : "light"} theme`} onClick={onToggleTheme}>
          {theme === "light" ? <Moon size={17} /> : <Sun size={17} />}
        </IconButton>
      </div>

      <section className="relative z-10 m-auto w-full max-w-[420px]">
        <header className="mb-7 text-center">
          <BrandLockup className="w-[190px]" />
          <p className="mx-auto mt-4 max-w-sm text-sm leading-6 text-muted">
            Your private notes, files, and agents behind one trusted gate.
          </p>
        </header>

        <Card
          as="form"
          onSubmit={auth.handleSubmit}
          className="w-full border-border bg-surface p-6 shadow-[var(--ng-focus-shadow)] sm:p-7"
          aria-labelledby="auth-title"
        >
          <div className="mb-6">
            <p className="mb-2 text-xs font-semibold uppercase tracking-[0.16em] text-muted">Welcome back</p>
            <h1 id="auth-title" className="text-2xl font-semibold tracking-tight">Continue to NoteGate</h1>
            <p className="mt-2 text-sm leading-6 text-muted">Use the Google account connected to your NoteGate workspace.</p>
          </div>

          <button type="button" className="ng-google-button" onClick={auth.startLogin}>
            <GoogleMark />
            Continue with Google
          </button>

          <a
            href={auth.loginHref}
            target="notegate-login"
            onClick={() => auth.beginPolling(null)}
            className="mt-3 block rounded text-center text-xs text-muted underline underline-offset-2 hover:text-text"
          >
            Open Google sign-in in a new window
          </a>

          <div className="mt-6 flex gap-2.5 rounded-xl border border-border bg-panel p-3 text-xs leading-5 text-muted">
            <ShieldCheck className="mt-0.5 text-success" size={16} aria-hidden="true" />
            <p>Google SSO is NoteGate's only production sign-in method. NoteGate never asks for your Google password.</p>
          </div>

          {auth.loginHint ? (
            <Card className="mt-3 text-xs leading-5 text-muted" padding="sm" role="status" aria-live="polite">
              {auth.loginHint}
            </Card>
          ) : null}

          {devApiKeyFallbackEnabled ? (
            <details className="mt-5 rounded-xl border border-border bg-panel p-3">
              <summary className="cursor-pointer rounded text-sm font-medium">Developer API key fallback</summary>
              <TextField
                id="api-key"
                name="apiKey"
                label="User API key"
                value={auth.apiKey}
                onChange={(event) => auth.setApiKey(event.target.value)}
                inputClassName="mt-2 bg-bg font-mono text-sm"
                placeholder="ngk_v1_..."
                autoComplete="off"
              />
              <Button type="submit" className="mt-4 w-full" secondary disabled={!auth.canSubmitApiKey}>Open with API key</Button>
            </details>
          ) : null}
        </Card>
      </section>
    </main>
  );
}

function AuthBackdropMark() {
  return (
    <svg className="ng-auth-backdrop-mark" viewBox="0 0 512 512" aria-hidden="true">
      <g fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round">
        <path d="M116 410V102a30 30 0 0 1 30-30h210" strokeWidth="24" />
        <path d="M116 410h240" strokeWidth="24" />
        <path d="M198 176v160M198 228h80M198 304h80" strokeWidth="18" />
      </g>
      <g fill="currentColor">
        <rect x="174" y="150" width="48" height="48" rx="9" />
        <rect x="270" y="204" width="58" height="48" rx="9" />
        <rect x="270" y="280" width="58" height="48" rx="9" />
        <path d="M342 112 430 66v380l-88-46V112Z" />
      </g>
    </svg>
  );
}

function GoogleMark() {
  return <img src="/google-g.png" className="size-5 object-contain" alt="" aria-hidden="true" />;
}
