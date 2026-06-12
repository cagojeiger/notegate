import { useState } from "react";

import { ApiProvider } from "../api/ApiProvider";
import { DevAuthGate } from "../auth/DevAuthGate";
import { readDevApiKey } from "../auth/session";
import { AppShell } from "../layout/AppShell";

export function App() {
  const [apiKey, setApiKey] = useState(() => readDevApiKey());

  if (!apiKey) {
    return <DevAuthGate onAuthenticated={setApiKey} />;
  }

  return (
    <ApiProvider apiKey={apiKey}>
      <AppShell onSignOut={() => setApiKey(null)} />
    </ApiProvider>
  );
}
