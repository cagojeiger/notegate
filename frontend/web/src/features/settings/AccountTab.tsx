import type { Me } from "../../api/types";
import { Button, Card, HelpTooltip, SectionHeader } from "../../shared/ui";
import { SettingsControls } from "./SettingsInspector";
import { EndpointRow } from "./EndpointRow";
import { AuthAccessManager } from "./AuthAccessManager";

export function AccountTab({ me, onSignOut }: { me: Me | undefined; onSignOut: () => void }) {
  const origin = typeof window === "undefined" ? "" : window.location.origin;

  return (
    <div className="ng-settings-detail-view flex h-full min-h-0 flex-col gap-6">
      <div className="ng-settings-grid shrink-0">
        <section className="flex h-full flex-col">
          <SectionHeader title="Account" />
          <Card className="flex-1 text-sm">
            <div className="font-medium">{me?.account.display_name ?? "…"}</div>
            <div className="text-muted">{me?.user?.email ?? me?.account.kind ?? ""}</div>
          </Card>
        </section>

        <section className="flex h-full flex-col">
          <SectionHeader title="User MCP" />
          <Card className="flex-1 text-sm">
            <div className="flex items-end gap-1">
              <div className="min-w-0 flex-1"><EndpointRow label="Server URL" value={`${origin}/mcp`} copyLabel="Copy user MCP server URL" /></div>
              <HelpTooltip label="About User MCP" align="end">Connect as your account with OAuth 2.1. Your MCP client opens browser login and requests access.</HelpTooltip>
            </div>
          </Card>
        </section>
      </div>
      {me?.account.kind === "user" ? <AuthAccessManager onSignOut={onSignOut} /> : <SettingsControls><section><SectionHeader title="This browser" description="Signing out leaves connected applications and agent keys active." /><Button variant="danger" onClick={onSignOut}>Sign out</Button></section></SettingsControls>}
    </div>
  );
}
