import { ExternalLink } from "lucide-react";

import { Card, SectionHeader } from "../../shared/ui";
import { EndpointRow } from "./EndpointRow";

export function AgentConnections() {
  const origin = typeof window === "undefined" ? "" : window.location.origin;

  return (
    <section>
      <SectionHeader title="Connections" description="Shared endpoints for every agent." />
      <Card className="space-y-4 text-sm">
        <EndpointRow label="Agent MCP" value={`${origin}/mcp`} copyLabel="Copy Agent MCP server URL" />
        <EndpointRow label="REST API" value={`${origin}/api/v2`} copyLabel="Copy API base URL" />
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
  );
}
