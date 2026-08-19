import { Moon, Sun } from "lucide-react";

import type { Me } from "../../api/types";
import { Button, Card, SectionHeader } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { EndpointRow } from "./EndpointRow";

export function AccountTab({ me, onSignOut }: { me: Me | undefined; onSignOut: () => void }) {
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const origin = typeof window === "undefined" ? "" : window.location.origin;

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
        <button type="button" onClick={toggleTheme} className="flex w-full items-center justify-between rounded-xl border border-border bg-surface p-4 text-workbench transition hover:bg-panel">
          <span>Theme</span>
          <span className="flex items-center gap-2 capitalize text-muted">{theme === "light" ? <Sun size={16} /> : <Moon size={16} />} {theme}</span>
        </button>
      </section>

      <section>
        <SectionHeader title="User MCP" description="Connect as your account with OAuth 2.1." />
        <Card className="space-y-3 text-sm">
          <EndpointRow label="Server URL" value={`${origin}/mcp`} copyLabel="Copy user MCP server URL" />
          <p className="text-xs leading-5 text-muted">Your MCP client opens browser login and requests access as your user account.</p>
        </Card>
      </section>

      <Button variant="danger" className="w-full" onClick={onSignOut}>Sign out</Button>
    </div>
  );
}
