import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { ApiProvider, useApiClient } from "../api/ApiProvider";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";
import { DevAuthGate } from "../auth/DevAuthGate";
import { readDevApiKey } from "../auth/session";
import { AppShell } from "../layout/AppShell";
import { FullScreenStatus } from "../layout/FullScreenStatus";

export function App() {
  const [apiKey, setApiKey] = useState(() => readDevApiKey());

  return (
    <ApiProvider apiKey={apiKey}>
      <AuthBoundary apiKey={apiKey} onAuthenticated={setApiKey} onSignOut={() => setApiKey(null)} />
    </ApiProvider>
  );
}

function AuthBoundary({ apiKey, onAuthenticated, onSignOut }: { apiKey: string | null; onAuthenticated: (apiKey: string) => void; onSignOut: () => void }) {
  const client = useApiClient();
  const meQuery = useQuery({ queryKey: [...queryKeys.me, apiKey], queryFn: () => getMe(client), retry: false });

  if (!meQuery.isFetched && meQuery.isLoading) return <FullScreenStatus label="Checking session" />;
  if (!meQuery.data) {
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
