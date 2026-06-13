import { useQuery } from "@tanstack/react-query";
import { Moon, Sun, X } from "lucide-react";

import { useApiClient } from "../api/ApiProvider";
import { getMe } from "../api/me";
import { queryKeys } from "../api/queryKeys";
import { useUiStore } from "../stores/uiStore";

export function SettingsModal({ onClose, onSignOut }: { onClose: () => void; onSignOut: () => void }) {
  const client = useApiClient();
  const meQuery = useQuery({ queryKey: queryKeys.me, queryFn: () => getMe(client) });
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const me = meQuery.data;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <button type="button" aria-label="Close settings" className="absolute inset-0 bg-black/40" onClick={onClose} />
      <div className="relative w-full max-w-md rounded-2xl border border-border bg-panel p-6 shadow-2xl">
        <div className="mb-5 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Settings</h2>
          <button type="button" aria-label="Close" onClick={onClose} className="grid size-8 place-items-center rounded-lg text-muted hover:bg-surface hover:text-text"><X size={16} /></button>
        </div>
        <section className="mb-4">
          <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Account</h3>
          <div className="rounded-xl border border-border bg-surface p-4 text-sm">
            <div className="font-medium">{me?.account.display_name ?? "…"}</div>
            <div className="text-muted">{me?.user?.email ?? me?.account.kind ?? ""}</div>
          </div>
        </section>
        <section className="mb-5">
          <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Appearance</h3>
          <button type="button" onClick={toggleTheme} className="flex w-full items-center justify-between rounded-xl border border-border bg-surface p-4 text-sm hover:bg-panel">
            <span>Theme</span>
            <span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
          </button>
        </section>
        <button type="button" onClick={onSignOut} className="w-full rounded-lg border border-danger/40 px-4 py-2 text-sm font-semibold text-danger transition hover:bg-danger/10">Sign out</button>
      </div>
    </div>
  );
}
