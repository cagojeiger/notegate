import { ExternalLink } from "lucide-react";

import { Card, SectionHeader } from "../../shared/ui";
import { EndpointRow } from "./EndpointRow";

export function ConnectionsTab({ canManageAgents }: { canManageAgents: boolean }) {
  const origin = typeof window === "undefined" ? "" : window.location.origin;

  return (
    <div className="space-y-4">
      <section>
        <SectionHeader title="User MCP" description="Connect as your account with OAuth 2.1." />
        <Card className="space-y-3 text-sm">
          <EndpointRow label="Server URL" value={`${origin}/mcp`} copyLabel="Copy user MCP server URL" />
          <p className="text-xs leading-5 text-muted">Your MCP client opens browser login and requests access as your user account.</p>
        </Card>
      </section>

      {canManageAgents ? (
        <>
          <section>
            <SectionHeader title="Agent MCP" description="Connect with an API key created for an agent." />
            <Card className="text-sm">
              <EndpointRow label="Server URL" value={`${origin}/mcp/v2`} copyLabel="Copy Agent MCP server URL" />
            </Card>
          </section>

          <section>
            <SectionHeader title="REST API" description="Call the public API with an Agent API key." />
            <Card className="space-y-3 text-sm">
              <EndpointRow label="Base URL" value={`${origin}/api/v2`} copyLabel="Copy API base URL" />
              <a
                href={`${origin}/swagger-ui/v2/`}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm font-medium text-primary outline-none transition hover:bg-[var(--ng-hover)] focus-visible:ring-2 focus-visible:ring-primary/45"
              >
                API documentation <ExternalLink size={14} />
              </a>
            </Card>
          </section>
        </>
      ) : null}
    </div>
  );
}
