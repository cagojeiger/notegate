import { useCallback, useEffect, useState } from "react";

import { ApiProvider } from "../api/ApiProvider";
import { ApiError } from "../api/errors";
import { DevAuthGate } from "../auth/DevAuthGate";
import { useSessionQuery } from "../auth/useAuthQueries";
import { clearDevApiKey, readDevApiKey } from "../auth/session";
import { AppShell } from "../layout/AppShell";
import { FullScreenStatus } from "../layout/FullScreenStatus";

export function App() {
  const [apiKey, setApiKey] = useState(() => readDevApiKey());
  const [sessionRevision, setSessionRevision] = useState(0);

  // A 401 from any non-session query, or an explicit sign-out, means the
  // authenticated session is no longer trustworthy. Bump the revision so /me
  // is checked with a fresh query key instead of reusing stale account data.
  const resetSession = useCallback(() => {
    clearDevApiKey();
    setApiKey(null);
    setSessionRevision((revision) => revision + 1);
  }, []);

  const handleApiKeyAuthenticated = useCallback((nextApiKey: string) => {
    setApiKey(nextApiKey);
    setSessionRevision((revision) => revision + 1);
  }, []);

  return (
    <ApiProvider apiKey={apiKey} onUnauthorized={resetSession}>
      <AuthBoundary apiKey={apiKey} sessionRevision={sessionRevision} onAuthenticated={handleApiKeyAuthenticated} onSignOut={resetSession} />
    </ApiProvider>
  );
}

function AuthBoundary({
  apiKey,
  sessionRevision,
  onAuthenticated,
  onSignOut
}: {
  apiKey: string | null;
  sessionRevision: number;
  onAuthenticated: (apiKey: string) => void;
  onSignOut: () => void;
}) {
  const meQuery = useSessionQuery(apiKey, sessionRevision);

  // react-query keeps the last good `data` on error, so an expired session
  // surfaces as a 401 error alongside stale data — treat that as logged-out.
  const unauthorized = meQuery.error instanceof ApiError && meQuery.error.status === 401;

  useEffect(() => {
    if (unauthorized && apiKey) onSignOut();
  }, [apiKey, onSignOut, unauthorized]);

  if (!meQuery.isFetched && meQuery.isLoading) return <FullScreenStatus label="Checking session" />;

  if (!meQuery.data || unauthorized) {
    return (
      <DevAuthGate
        onAuthenticated={onAuthenticated}
        onSessionAuthenticated={async () => {
          const result = await meQuery.refetch();
          return result.isSuccess;
        }}
      />
    );
  }

  return <AppShell onSignOut={onSignOut} />;
}
