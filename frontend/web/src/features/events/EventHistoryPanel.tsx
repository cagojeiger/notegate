import { useEffect, useMemo } from "react";

import type { Space } from "../../api/types";
import { SectionHeader, Tabs } from "../../shared/ui";
import { SettingsControls, useSettingsInspector, useSettingsNavigationScroll } from "../settings/SettingsInspector";
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
  const inspector = useSettingsInspector();
  useSettingsNavigationScroll("history");
  const [tab, setTab] = useSettingsViewState<HistoryTab>("history-kind", "files");
  const tabs = useMemo(
    () => TABS.filter((item) => item.id === "files" || canViewAuditEvents),
    [canViewAuditEvents]
  );

  useEffect(() => {
    if (!canViewAuditEvents && tab !== "files") setTab("files");
  }, [canViewAuditEvents, tab, setTab]);

  return (
    <SettingsPage label="History" title={tabs.find((item) => item.id === tab)?.label ?? "Changes"}
      inspectorLabel="Choose history type" scrollKey={`history-${tab}`}
      contentLabel={`${tabs.find((item) => item.id === tab)?.label ?? "Changes"} history`}>
      <SettingsControls slot="navigation"><SectionHeader title="History type" /><Tabs items={tabs} value={tab} onChange={(value) => { setTab(value); inspector?.onNavigate(); }} label="History sections" orientation={inspector ? "vertical" : "horizontal"} /></SettingsControls>
        {canViewAuditEvents && tab === "audit" ? <AuditEventsPanel /> : null}
        {canViewAuditEvents && tab === "mcp" ? <CommandInvocationsPanel surface="mcp" /> : null}
        {canViewAuditEvents && tab === "cli" ? <CommandInvocationsPanel surface="cli" /> : null}
        {canViewAuditEvents && tab === "api" ? <CommandInvocationsPanel surface="api" /> : null}
        {canViewAuditEvents && tab === "jobs" ? <BackgroundJobsPanel /> : null}
        {tab === "files" ? <FileChangeEventsPanel spaces={spaces} initialSpaceId={initialSpaceId} canViewOwnedHistory={canViewAuditEvents} /> : null}
    </SettingsPage>
  );
}
