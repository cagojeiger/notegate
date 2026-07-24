import { useCallback, useEffect, useState } from "react";

import { ApiProvider } from "../api/ApiProvider";
import { ApiError } from "../api/errors";
import type { Me } from "../entities/account/model";
import { DevAuthGate } from "../auth/DevAuthGate";
import { useSessionQuery } from "../auth/useAuthQueries";
import { clearDevApiKey, readDevApiKey } from "../auth/session";
import type { ThemeMode } from "../design/tokens";
import { UploadProvider } from "../features/uploads/UploadProvider";
import { AppShell } from "../layout/AppShell";
import { FullScreenStatus } from "../layout/FullScreenStatus";
import { Button } from "../shared/ui";
import { persistTheme, useUiStore } from "../stores/uiStore";
import { clearAuthenticatedClientState, resetWorkbenchClientState } from "./clientSession";

const DEV_API_KEY_FALLBACK_ENABLED =
  import.meta.env.DEV || import.meta.env.MODE === "test" || import.meta.env.VITE_NOTEGATE_ENABLE_DEV_API_KEY === "true";

export function App() {
  const [apiKey, setApiKey] = useState(() => {
    if (DEV_API_KEY_FALLBACK_ENABLED) return readDevApiKey();
    clearDevApiKey();
    return null;
  });
  const [sessionRevision, setSessionRevision] = useState(0);
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const showToast = useUiStore((state) => state.showToast);

  useEffect(() => {
    persistTheme(theme);
  }, [theme]);

  const clearClientSession = useCallback(() => {
    clearAuthenticatedClientState();
  }, []);

  // A 401 from any non-session query, or an explicit sign-out, means the
  // authenticated session is no longer trustworthy. Bump the revision so /me
  // is checked with a fresh query key instead of reusing stale account data.
  const resetSession = useCallback(() => {
    clearClientSession();
    setApiKey(null);
    setSessionRevision((revision) => revision + 1);
  }, [clearClientSession]);

  const handleApiKeyAuthenticated = useCallback((nextApiKey: string) => {
    resetWorkbenchClientState();
    setApiKey(nextApiKey);
    setSessionRevision((revision) => revision + 1);
  }, []);

  const handleBrowserSessionAuthenticated = useCallback(() => {
    resetWorkbenchClientState();
    setSessionRevision((revision) => revision + 1);
  }, []);

  const authCacheKey = `${apiKey ?? "browser-session"}:${sessionRevision}`;

  return (
    <ApiProvider
      apiKey={apiKey}
      authCacheKey={authCacheKey}
      onUnauthorized={resetSession}
      onMutationError={showToast}
    >
      <AuthBoundary
        apiKey={apiKey}
        sessionRevision={sessionRevision}
        theme={theme}
        devApiKeyFallbackEnabled={DEV_API_KEY_FALLBACK_ENABLED}
        onToggleTheme={toggleTheme}
        onAuthenticated={handleApiKeyAuthenticated}
        onBrowserSessionAuthenticated={handleBrowserSessionAuthenticated}
        onBrowserSessionInvalidated={clearClientSession}
        onSignOut={resetSession}
      />
    </ApiProvider>
  );
}

function AuthBoundary({
  apiKey,
  sessionRevision,
  theme,
  devApiKeyFallbackEnabled,
  onToggleTheme,
  onAuthenticated,
  onBrowserSessionAuthenticated,
  onBrowserSessionInvalidated,
  onSignOut
}: {
  apiKey: string | null;
  sessionRevision: number;
  theme: ThemeMode;
  devApiKeyFallbackEnabled: boolean;
  onToggleTheme: () => void;
  onAuthenticated: (apiKey: string) => void;
  onBrowserSessionAuthenticated: () => void;
  onBrowserSessionInvalidated: () => void;
  onSignOut: () => void;
}) {
  const meQuery = useSessionQuery(apiKey, sessionRevision);
  const me = meQuery.data;
  const unauthorized = isUnauthorizedSession(meQuery.error);
  const authViewState = deriveAuthViewState({
    error: meQuery.error,
    isFetched: meQuery.isFetched,
    isLoading: meQuery.isLoading,
    session: me
  });

  useEffect(() => {
    if (!unauthorized) return;
    if (apiKey) onSignOut();
    else onBrowserSessionInvalidated();
  }, [apiKey, onBrowserSessionInvalidated, onSignOut, unauthorized]);

  if (authViewState.kind === "checking") return <FullScreenStatus label="Checking session" />;

  if (authViewState.kind === "temporarilyUnavailable") {
    return (
      <FullScreenStatus
        variant="status"
        label="Authentication temporarily unavailable"
        detail="Your session was not cleared. Try again once the auth service is reachable."
        action={
          <Button onClick={() => void meQuery.refetch()} disabled={meQuery.isFetching}>
            Retry
          </Button>
        }
      />
    );
  }

  if (authViewState.kind === "login") {
    return (
      <DevAuthGate
        devApiKeyFallbackEnabled={devApiKeyFallbackEnabled}
        theme={theme}
        onToggleTheme={onToggleTheme}
        onAuthenticated={onAuthenticated}
        onSessionAuthenticated={async () => {
          const result = await meQuery.refetch();
          if (result.isSuccess) onBrowserSessionAuthenticated();
          return result.isSuccess;
        }}
      />
    );
  }

  return (
    <UploadProvider>
      <AppShell me={authViewState.me} onSignOut={onSignOut} />
    </UploadProvider>
  );
}

type AuthViewState =
  | { kind: "checking" }
  | { kind: "temporarilyUnavailable" }
  | { kind: "login" }
  | { kind: "authenticated"; me: Me };

function deriveAuthViewState({
  error,
  isFetched,
  isLoading,
  session
}: {
  error: unknown;
  isFetched: boolean;
  isLoading: boolean;
  session: Me | undefined;
}): AuthViewState {
  if (!isFetched && isLoading) return { kind: "checking" };
  if (isUnauthorizedSession(error)) return { kind: "login" };
  if (!session && isTemporarilyUnavailable(error)) return { kind: "temporarilyUnavailable" };
  if (!session) return { kind: "login" };
  return { kind: "authenticated", me: session };
}

function isUnauthorizedSession(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

function isTemporarilyUnavailable(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 503 || error.kind === "auth_unavailable");
}
