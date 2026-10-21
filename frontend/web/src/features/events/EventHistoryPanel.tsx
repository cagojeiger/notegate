import { useEffect, useMemo } from "react";

import type { Space } from "../../api/types";
import { SelectField } from "../../shared/ui";
import { AuditEventsPanel } from "./AuditHistoryPanel";
import { FileChangeEventsPanel } from "./ChangesHistoryPanel";
import { BackgroundJobsPanel } from "./JobsHistoryPanel";
import { CommandInvocationsPanel } from "./InvocationHistoryPanel";

import { SettingsPage } from "../settings/SettingsPage";
import { useSettingsViewState } from "../settings/settingsViewState";

type HistoryTab = "audit" | "files" | "mcp" | "cli" | "api" | "jobs";

const TABS: { id: HistoryTab; label: string }[] = [
  { id: "files", label: "Changes" },
  { id: "audit", label: "Audit" },
  { id: "mcp", label: "MCP" },
  { id: "cli", label: "CLI" },
  { id: "api", label: "API" },
  { id: "jobs", label: "Jobs" }
];

export function EventHistoryPanel({
  spaces,
  initialSpaceId,
  canViewAuditEvents
}: {
  spaces: Space[];
  initialSpaceId: string | null;
  canViewAuditEvents: boolean;
}) {
  const [tab, setTab] = useSettingsViewState<HistoryTab>("history-kind", "files");
  const tabs = useMemo(
    () => TABS.filter((item) => item.id === "files" || canViewAuditEvents),
    [canViewAuditEvents]
  );

  useEffect(() => {
    if (!canViewAuditEvents && tab !== "files") setTab("files");
  }, [canViewAuditEvents, tab, setTab]);

  return (
    <SettingsPage label="History" title="History"
      inspectorLabel={canViewAuditEvents && ["mcp", "cli", "api"].includes(tab) ? "Open call details" : undefined} scrollKey={`history-${tab}`}
      contentLabel={`${tabs.find((item) => item.id === tab)?.label ?? "Changes"} history`}>
      <div className="flex min-w-0 flex-wrap items-end gap-2">
        {/* Keep this control mounted while each panel supplies its filters, refresh and results. */}
        <SelectField label="History type" className="w-32 min-w-0 shrink-0 sm:w-40"
          value={tab} onChange={(event) => setTab(event.target.value as HistoryTab)}>
          {tabs.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
        </SelectField>
        {canViewAuditEvents && tab === "audit" ? <AuditEventsPanel /> : null}
        {canViewAuditEvents && tab === "mcp" ? <CommandInvocationsPanel surface="mcp" /> : null}
        {canViewAuditEvents && tab === "cli" ? <CommandInvocationsPanel surface="cli" /> : null}
        {canViewAuditEvents && tab === "api" ? <CommandInvocationsPanel surface="api" /> : null}
        {canViewAuditEvents && tab === "jobs" ? <BackgroundJobsPanel /> : null}
        {tab === "files" ? <FileChangeEventsPanel spaces={spaces} initialSpaceId={initialSpaceId} canViewOwnedHistory={canViewAuditEvents} /> : null}
      </div>
    </SettingsPage>
  );
}
