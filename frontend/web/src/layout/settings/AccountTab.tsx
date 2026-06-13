import { useQuery } from "@tanstack/react-query";
import { Moon, Sun } from "lucide-react";

import { useApiClient } from "../../api/ApiProvider";
import { getMe } from "../../api/me";
import { queryKeys } from "../../api/queryKeys";
import { useUiStore } from "../../stores/uiStore";

export function AccountTab({ onSignOut }: { onSignOut: () => void }) {
  const client = useApiClient();
  const meQuery = useQuery({ queryKey: queryKeys.me, queryFn: () => getMe(client) });
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const me = meQuery.data;
  return (
    <div className="space-y-4">
      <section>
        <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Account</h3>
        <div className="rounded-xl border border-border bg-surface p-4 text-sm">
          <div className="font-medium">{me?.account.display_name ?? "…"}</div>
          <div className="text-muted">{me?.user?.email ?? me?.account.kind ?? ""}</div>
        </div>
      </section>
      <section>
        <h3 className="mb-2 text-xs font-bold uppercase tracking-wide text-muted">Appearance</h3>
        <button type="button" onClick={toggleTheme} className="flex w-full items-center justify-between rounded-xl border border-border bg-surface p-4 text-sm hover:bg-panel">
          <span>Theme</span>
          <span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
        </button>
      </section>
      <button type="button" onClick={onSignOut} className="w-full rounded-lg border border-danger/40 px-4 py-2 text-sm font-semibold text-danger transition hover:bg-danger/10">Sign out</button>
    </div>
  );
}
