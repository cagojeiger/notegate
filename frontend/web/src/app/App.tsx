import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { ApiProvider, useApiClient } from "../api/ApiProvider";
import { ApiError } from "../api/errors";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";
import { DevAuthGate } from "../auth/DevAuthGate";
import { clearDevApiKey, readDevApiKey } from "../auth/session";
import { AppShell } from "../layout/AppShell";
import { FullScreenStatus } from "../layout/FullScreenStatus";

export function App() {
  const [apiKey, setApiKey] = useState(() => readDevApiKey());

  // A 401 from anywhere means the key/session died; drop it so the gate shows.
  function handleUnauthorized() {
    clearDevApiKey();
    setApiKey(null);
  }

  return (
    <ApiProvider apiKey={apiKey} onUnauthorized={handleUnauthorized}>
      <AuthBoundary apiKey={apiKey} onAuthenticated={setApiKey} onSignOut={handleUnauthorized} />
    </ApiProvider>
  );
}

function AuthBoundary({ apiKey, onAuthenticated, onSignOut }: { apiKey: string | null; onAuthenticated: (apiKey: string) => void; onSignOut: () => void }) {
  const client = useApiClient();
  const meQuery = useQuery({ queryKey: [...queryKeys.me, apiKey], queryFn: () => getMe(client), retry: false });

  if (!meQuery.isFetched && meQuery.isLoading) return <FullScreenStatus label="Checking session" />;
  // react-query keeps the last good `data` on error, so an expired session
  // surfaces as a 401 error alongside stale data — treat that as logged-out.
  const unauthorized = meQuery.error instanceof ApiError && meQuery.error.status === 401;
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
