import { useEffect, useMemo, useState } from "react";

import type { Space } from "../../api/types";
import { Modal, Tabs } from "../../shared/ui";
import { AuditEventsPanel } from "./AuditHistoryPanel";
import { FileChangeEventsPanel } from "./ChangesHistoryPanel";
import { BackgroundJobsPanel } from "./JobsHistoryPanel";
import { CommandInvocationsPanel } from "./InvocationHistoryPanel";

type HistoryTab = "audit" | "files" | "mcp" | "cli" | "jobs";

const TABS: { id: HistoryTab; label: string }[] = [
  { id: "files", label: "Changes" },
  { id: "audit", label: "Audit" },
  { id: "mcp", label: "MCP" },
  { id: "cli", label: "CLI" },
  { id: "jobs", label: "Jobs" }
];

export function EventHistoryModal({
  spaces,
  initialSpaceId,
  canViewAuditEvents,
  onClose
}: {
  spaces: Space[];
  initialSpaceId: string | null;
  canViewAuditEvents: boolean;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<HistoryTab>("files");
  const tabs = useMemo(
    () => TABS.filter((item) => item.id === "files" || canViewAuditEvents),
    [canViewAuditEvents]
  );

  useEffect(() => {
    if (!canViewAuditEvents && tab !== "files") setTab("files");
  }, [canViewAuditEvents, tab]);

  return (
    <Modal title="History" onClose={onClose} width="max-w-5xl">
      <Tabs items={tabs} value={tab} onChange={setTab} label="History sections" />
      <div className="min-h-[20rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1 sm:min-h-[24rem]">
        {canViewAuditEvents && tab === "audit" ? <AuditEventsPanel /> : null}
        {canViewAuditEvents && tab === "mcp" ? <CommandInvocationsPanel surface="mcp" /> : null}
        {canViewAuditEvents && tab === "cli" ? <CommandInvocationsPanel surface="cli" /> : null}
        {canViewAuditEvents && tab === "jobs" ? <BackgroundJobsPanel /> : null}
        {tab === "files" ? <FileChangeEventsPanel spaces={spaces} initialSpaceId={initialSpaceId} /> : null}
      </div>
    </Modal>
  );
}
