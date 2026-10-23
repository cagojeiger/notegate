import { lazy, Suspense, useCallback, useEffect, useState } from "react";

import { ApiProvider } from "../api/ApiProvider";
import { ApiError } from "../api/errors";
import type { Me } from "../api/types";
import { LoginGate } from "../auth/LoginGate";
import { useSessionQuery } from "../auth/useAuthQueries";
import { useBrowserSessionActivity } from "../auth/useBrowserSessionActivity";
import { PublicInformationPage } from "../features/public-information/PublicInformationPage";
import { FullScreenStatus } from "../layout/FullScreenStatus";
import { Button } from "../shared/ui";
import { useUiStore } from "../stores/uiStore";

const AuthenticatedWorkspace = lazy(() => import("./AuthenticatedWorkspace"));

export function App() {
  switch (window.location.pathname) {
    case "/terms": return <PublicInformationPage page="terms" />;
    case "/privacy": return <PublicInformationPage page="privacy" />;
    case "/support": return <PublicInformationPage page="support" />;
    default: return <SessionApp />;
  }
}

function SessionApp() {
  const [sessionRevision, setSessionRevision] = useState(0);
  const showToast = useUiStore((state) => state.showToast);

  const refreshSession = useCallback(() => {
    useUiStore.getState().resetSessionWorkspace();
    setSessionRevision((revision) => revision + 1);
  }, []);

  const authCacheKey = `browser-session:${sessionRevision}`;

  return (
    <ApiProvider
      authCacheKey={authCacheKey}
      onUnauthorized={refreshSession}
      onMutationError={showToast}
    >
      <AuthBoundary
        key={sessionRevision}
        sessionRevision={sessionRevision}
        onSessionChanged={refreshSession}
      />
    </ApiProvider>
  );
}

function AuthBoundary({
  sessionRevision,
  onSessionChanged
}: {
  sessionRevision: number;
  onSessionChanged: () => void;
}) {
  const meQuery = useSessionQuery(sessionRevision);
  const me = meQuery.data;
  const authViewState = deriveAuthViewState({
    error: meQuery.error,
    isFetched: meQuery.isFetched,
    isLoading: meQuery.isLoading,
    session: me
  });
  const [cacheOwner, setCacheOwner] = useState<string | null>(null);
  const accountId = authViewState.kind === "authenticated" ? authViewState.me.account.id : null;
  const replaceCache = cacheOwner !== null && (authViewState.kind === "login" || (accountId !== null && accountId !== cacheOwner));
  useEffect(() => {
    if (replaceCache) onSessionChanged();
    else if (accountId !== null && cacheOwner === null) setCacheOwner(accountId);
  }, [accountId, cacheOwner, replaceCache, onSessionChanged]);
  useBrowserSessionActivity(authViewState.kind === "authenticated" && accountId === cacheOwner, onSessionChanged);

  // A different successful /me response must never render against the previous owner's cache.
  if (authViewState.kind === "checking" || replaceCache || (accountId !== null && cacheOwner === null)) return <FullScreenStatus label="Checking session" />;

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
      <LoginGate
        onSessionAuthenticated={async () => {
          const result = await meQuery.refetch();
          if (result.isSuccess) {
            onSessionChanged();
          }
          return result.isSuccess;
        }}
      />
    );
  }

  return (
    <Suspense fallback={<FullScreenStatus label="Loading workspace" />}>
      <AuthenticatedWorkspace me={authViewState.me} onSignOut={onSessionChanged} />
    </Suspense>
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
