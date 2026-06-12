import { FormEvent, useState } from "react";

import { writeDevApiKey } from "./session";

type DevAuthGateProps = {
  onAuthenticated: (apiKey: string) => void;
};

export function DevAuthGate({ onAuthenticated }: DevAuthGateProps) {
  const [apiKey, setApiKey] = useState("");

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = apiKey.trim();
    if (!trimmed) return;
    writeDevApiKey(trimmed);
    onAuthenticated(trimmed);
  }

  return (
    <main className="grid h-full place-items-center bg-bg px-6 text-text">
      <form onSubmit={handleSubmit} className="w-full max-w-md rounded-2xl border border-border bg-panel p-6 shadow-2xl shadow-black/20">
        <div className="mb-6 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-primary text-lg font-bold text-bg">N</div>
          <div>
            <h1 className="text-xl font-semibold">Notegate dev access</h1>
            <p className="text-sm text-muted">Paste a user API key to open the dashboard.</p>
          </div>
        </div>
        <label className="block text-sm font-medium" htmlFor="api-key">
          User API key
        </label>
        <input
          id="api-key"
          name="apiKey"
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
          className="mt-2 w-full rounded-lg border border-border bg-surface px-3 py-2 font-mono text-sm outline-none ring-primary/0 transition focus:border-primary focus:ring-2 focus:ring-primary/30"
          placeholder="ng_user_..."
          autoComplete="off"
          autoFocus
        />
        <p className="mt-3 text-xs leading-5 text-muted">The key is stored in sessionStorage only. OAuth login will replace this temporary gate later.</p>
        <button
          type="submit"
          className="mt-6 w-full rounded-lg bg-primary px-4 py-2 text-sm font-semibold text-bg transition hover:bg-[var(--ng-primary-hover)] disabled:cursor-not-allowed disabled:opacity-50"
          disabled={!apiKey.trim()}
        >
          Open dashboard
        </button>
      </form>
    </main>
  );
}
