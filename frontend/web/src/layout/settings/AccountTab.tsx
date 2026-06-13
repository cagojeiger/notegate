import { useQuery } from "@tanstack/react-query";
import { Moon, Sun } from "lucide-react";

import { useApiClient } from "../../api/ApiProvider";
import { getMe } from "../../api/me";
import { queryKeys } from "../../api/queryKeys";
import { Button, Card, SectionHeader } from "../../shared/ui";
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
        <SectionHeader title="Account" />
        <Card className="text-sm">
          <div className="font-medium">{me?.account.display_name ?? "…"}</div>
          <div className="text-muted">{me?.user?.email ?? me?.account.kind ?? ""}</div>
        </Card>
      </section>

      <section>
        <SectionHeader title="Appearance" />
        <button type="button" onClick={toggleTheme} className="flex w-full items-center justify-between rounded-xl border border-border bg-surface p-4 text-sm transition hover:bg-panel">
          <span>Theme</span>
          <span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
        </button>
      </section>

      <Button variant="danger" className="w-full" onClick={onSignOut}>Sign out</Button>
    </div>
  );
}
