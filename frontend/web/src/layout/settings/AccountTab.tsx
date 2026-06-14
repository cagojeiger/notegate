import { Copy, Moon, Sun } from "lucide-react";

import type { Me } from "../../api/types";
import { Button, Card, IconButton, SectionHeader } from "../../shared/ui";
import { KeyManager } from "./KeyManager";
import { useUiStore } from "../../stores/uiStore";
import { useMyKeyManagerProps } from "./useSettingsQueries";

export function AccountTab({ me, onSignOut }: { me: Me | undefined; onSignOut: () => void }) {
  const theme = useUiStore((state) => state.theme);
  const toggleTheme = useUiStore((state) => state.toggleTheme);
  const keyManagerProps = useMyKeyManagerProps();
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

      <McpConnectionGuide />

      <section>
        <SectionHeader title="My API Keys" description="User keys authenticate as your account." />
        <KeyManager {...keyManagerProps} emptyLabel="No user API keys." />
      </section>

      <Button variant="danger" className="w-full" onClick={onSignOut}>Sign out</Button>
    </div>
  );
}

function McpConnectionGuide() {
  const showToast = useUiStore((state) => state.showToast);
  const origin = typeof window === "undefined" ? "" : window.location.origin;
  const mcpUrl = `${origin}/mcp`;

  function copy(value: string) {
    void navigator.clipboard?.writeText(value);
    showToast("Copied");
  }

  return (
    <section>
      <SectionHeader title="MCP Connection" description="Use this endpoint from an MCP client or agent runtime." />
      <Card className="space-y-4 text-sm">
        <div>
          <div className="mb-1 text-xs font-semibold uppercase tracking-[0.16em] text-muted">Server URL</div>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-bg px-3 py-2 font-mono text-xs">{mcpUrl}</code>
            <IconButton label="Copy MCP server URL" onClick={() => copy(mcpUrl)}><Copy size={15} /></IconButton>
          </div>
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="rounded-xl border border-seam bg-bg/60 p-3">
            <div className="font-medium">OAuth login</div>
            <p className="mt-1 text-xs leading-5 text-muted">Use the server URL above. The client should open the browser login flow and authenticate as your user account.</p>
          </div>
          <div className="rounded-xl border border-seam bg-bg/60 p-3">
            <div className="font-medium">API key</div>
            <p className="mt-1 text-xs leading-5 text-muted">For non-interactive agents, create an agent key in Agents. User keys below also work, but act with your full user authority.</p>
          </div>
        </div>
      </Card>
    </section>
  );
}
