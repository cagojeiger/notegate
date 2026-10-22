import { ExternalLink } from "lucide-react";
import { useRef } from "react";

import type { Me } from "../../api/types";
import { Card, SectionHeader, Tabs } from "../../shared/ui";
import { AuthAccessManager } from "./AuthAccessManager";
import { EndpointRow } from "./EndpointRow";
import { SettingsPage } from "./SettingsPage";
import { useSettingsViewState } from "./settingsViewState";

type View = "guide" | "cli" | "mcp";
const VIEWS: { id: View; label: string }[] = [
  { id: "guide", label: "Connection guide" }, { id: "cli", label: "CLI" }, { id: "mcp", label: "MCP" }
];

export function ConnectionsTab({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
  const [view, setView] = useSettingsViewState<View>("connections-view", "guide");
  const navigationRef = useRef<HTMLDivElement>(null);
  const current = me.account.kind === "user" ? view : "guide";
  const navigation = <div ref={navigationRef}><Tabs label="Connection views" value={current} activationMode="manual" onChange={(value) => {
    setView(value);
    requestAnimationFrame(() => navigationRef.current?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus());
  }} items={me.account.kind === "user" ? VIEWS : VIEWS.slice(0, 1)} /></div>;

  if (current !== "guide") return <AuthAccessManager key={current} kind={current} onSignOut={onSignOut} navigation={navigation} />;
  const origin = window.location.origin;
  return <SettingsPage title="Connections" summary={navigation} scrollKey="connection-guide">
    <div className="ng-settings-grid">
      <section className="ng-settings-wide">
        <SectionHeader title="User MCP" description="Connect an application as your user account with OAuth 2.1." />
        <Card><EndpointRow label="Server URL" value={`${origin}/mcp`} copyLabel="Copy user MCP server URL" /></Card>
      </section>
      <section>
        <SectionHeader title="Agent MCP" description="Connect with an agent API key and its space permissions." />
        <Card><EndpointRow label="Server URL" value={`${origin}/mcp/v2`} copyLabel="Copy Agent MCP server URL" /></Card>
      </section>
      <section>
        <SectionHeader title="REST API" description="Use an agent API key for API requests." />
        <Card><EndpointRow label="Base URL" value={`${origin}/api/v2`} copyLabel="Copy API base URL" /></Card>
      </section>
      <a href={`${origin}/swagger-ui/v2/`} target="_blank" rel="noopener noreferrer"
        className="ng-settings-wide inline-flex min-h-workbench-control w-fit items-center gap-2 rounded-workbench-surface text-workbench font-medium text-primary outline-none hover:bg-[var(--ng-hover)]">
        API documentation <ExternalLink size={14} />
      </a>
    </div>
  </SettingsPage>;
}
