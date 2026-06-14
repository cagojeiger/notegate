import { Copy } from "lucide-react";

import { Card, IconButton, SectionHeader } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

const AUTH_HEADER = "Authorization: Bearer <credential>";

export function McpTab() {
  const showToast = useUiStore((state) => state.showToast);
  const origin = typeof window === "undefined" ? "" : window.location.origin;
  const mcpUrl = `${origin}/mcp`;

  function copy(value: string) {
    void navigator.clipboard?.writeText(value);
    showToast("Copied");
  }

  return (
    <div className="space-y-4">
      <section>
        <SectionHeader title="MCP" description="External clients use one endpoint and one bearer header." />
        <Card className="space-y-4 text-sm">
          <CopyRow label="Server URL" value={mcpUrl} copyLabel="Copy MCP server URL" onCopy={() => copy(mcpUrl)} />
          <CopyRow label="Authorization header" value={AUTH_HEADER} copyLabel="Copy authorization header" onCopy={() => copy(AUTH_HEADER)} />
          <p className="text-xs leading-5 text-muted">Use this same header for OAuth tokens, user API keys, and agent API keys. Browser session cookies are not accepted on <code className="font-mono">/mcp</code>.</p>
        </Card>
      </section>

      <section>
        <SectionHeader title="Connection methods" />
        <div className="grid gap-3 lg:grid-cols-3">
          <MethodCard
            title="OAuth login"
            badge="Recommended"
            body="For interactive MCP clients. The client can start browser login automatically when no bearer credential is configured."
            result="Acts as your user account."
          />
          <MethodCard
            title="Agent API key"
            badge="Automation"
            body="For CI and background agents. Create the key in Agents and pass it as Authorization: Bearer ngk_v1_..."
            result="Acts as one agent and only sees connected spaces."
          />
          <MethodCard
            title="User API key"
            badge="Advanced"
            body="For trusted local tools. Create the key in Account and pass it as Authorization: Bearer ngk_v1_..."
            result="Acts as your full user account."
          />
        </div>
      </section>
    </div>
  );
}

function CopyRow({ label, value, copyLabel, onCopy }: { label: string; value: string; copyLabel: string; onCopy: () => void }) {
  return (
    <div>
      <div className="mb-1 text-xs font-semibold uppercase tracking-[0.16em] text-muted">{label}</div>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-bg px-3 py-2 font-mono text-xs">{value}</code>
        <IconButton label={copyLabel} onClick={onCopy}><Copy size={15} /></IconButton>
      </div>
    </div>
  );
}

function MethodCard({ title, badge, body, result }: { title: string; badge: string; body: string; result: string }) {
  return (
    <Card className="flex min-h-44 flex-col gap-3 text-sm">
      <div className="flex items-start justify-between gap-3">
        <div className="font-medium text-text">{title}</div>
        <span className="rounded-full border border-border bg-bg px-2 py-0.5 text-[11px] font-medium text-muted">{badge}</span>
      </div>
      <p className="text-xs leading-5 text-muted">{body}</p>
      <p className="mt-auto text-xs font-medium leading-5 text-text">{result}</p>
    </Card>
  );
}
