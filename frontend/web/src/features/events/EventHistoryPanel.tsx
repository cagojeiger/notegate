import { useEffect, useMemo } from "react";

import type { Space } from "../../api/types";
import { SectionHeader, Tabs } from "../../shared/ui";
import { SettingsControls, useSettingsInspector, useSettingsNavigationScroll } from "../settings/SettingsInspector";
import { AuditEventsPanel } from "./AuditHistoryPanel";
import { FileChangeEventsPanel } from "./ChangesHistoryPanel";
import { BackgroundJobsPanel } from "./JobsHistoryPanel";
import { CommandInvocationsPanel } from "./InvocationHistoryPanel";

import { SettingsViewHeader } from "../settings/SettingsViewHeader";
import { useSettingsScroll, useSettingsViewState } from "../settings/settingsViewState";

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
  const scroll = useSettingsScroll(`history-list-${tab}`);
  const tabs = useMemo(
    () => TABS.filter((item) => item.id === "files" || canViewAuditEvents),
    [canViewAuditEvents]
  );

  useEffect(() => {
    if (!canViewAuditEvents && tab !== "files") setTab("files");
  }, [canViewAuditEvents, tab, setTab]);

  return (
    <section aria-label="History" className="flex h-full min-h-0 flex-col">
      <SettingsControls slot="navigation"><SectionHeader title="History type" /><Tabs items={tabs} value={tab} onChange={(value) => { setTab(value); inspector?.onNavigate(); }} label="History sections" orientation={inspector ? "vertical" : "horizontal"} /></SettingsControls>
      <SettingsViewHeader title={tabs.find((item) => item.id === tab)?.label ?? "Changes"} inspectorLabel="Choose history type" />
      <div key={tab} ref={scroll} role="tabpanel" aria-label={`${tabs.find((item) => item.id === tab)?.label ?? "Changes"} history`} className="min-h-0 flex-1 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]">
        {canViewAuditEvents && tab === "audit" ? <AuditEventsPanel /> : null}
        {canViewAuditEvents && tab === "mcp" ? <CommandInvocationsPanel surface="mcp" /> : null}
        {canViewAuditEvents && tab === "cli" ? <CommandInvocationsPanel surface="cli" /> : null}
        {canViewAuditEvents && tab === "api" ? <CommandInvocationsPanel surface="api" /> : null}
        {canViewAuditEvents && tab === "jobs" ? <BackgroundJobsPanel /> : null}
        {tab === "files" ? <FileChangeEventsPanel spaces={spaces} initialSpaceId={initialSpaceId} canViewOwnedHistory={canViewAuditEvents} /> : null}
      </div>
    </section>
  );
}
