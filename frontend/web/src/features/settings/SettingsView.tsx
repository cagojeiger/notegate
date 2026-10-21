import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { X } from "lucide-react";

import type { Me, Space } from "../../api/types";
import { canManageAgents, canViewAuditEvents } from "../../auth/permissions";
import { IconButton, WorkbenchSurface } from "../../shared/ui";
import { EventHistoryPanel } from "../events/EventHistoryPanel";
import { TrashPanel } from "../trash/TrashPanel";
import { AccountTab } from "./AccountTab";
import { AgentsTab } from "./AgentsTab";
import { ConnectionsTab } from "./ConnectionsTab";
import { GeneralTab } from "./GeneralTab";
import { SettingsInspectorContext } from "./SettingsInspector";
import { SettingsViewSession, useSettingsScroll } from "./settingsViewState";
import "./settings.css";

type Tab = "general" | "account" | "connections" | "agents" | "history" | "trash";
const TABS: { id: Tab; label: string; controls: string }[] = [
  { id: "general", label: "General", controls: "settings-general-panel" },
  { id: "account", label: "Account", controls: "settings-account-panel" },
  { id: "connections", label: "Connections", controls: "settings-connections-panel" },
  { id: "agents", label: "Agents", controls: "settings-agents-panel" },
  { id: "history", label: "History", controls: "settings-history-panel" },
  { id: "trash", label: "Trash", controls: "settings-trash-panel" }
];

type SettingsViewProps = {
  me: Me;
  spaces?: Space[];
  initialSpaceId?: string | null;
  onClose: () => void;
  onSignOut: () => void;
  onResetSavedWorkspace?: () => void;
  isMobile?: boolean;
  inspectorOpen?: boolean;
  onOpenInspector?: () => void;
  onCloseInspector?: () => void;
  onInspectorAvailabilityChange?: (available: boolean) => void;
};

export function SettingsView(props: SettingsViewProps) {
  return <SettingsViewSession><SettingsWorkbench {...props} /></SettingsViewSession>;
}

function SettingsWorkbench({ me, spaces = [], initialSpaceId = null, onClose, onSignOut,
  onResetSavedWorkspace = () => undefined, isMobile = false, inspectorOpen = true,
  onOpenInspector = () => undefined, onCloseInspector = () => undefined, onInspectorAvailabilityChange
}: SettingsViewProps) {
  const [tab, setTab] = useState<Tab>("general");
  const inspectorId = useId();
  const [container, setContainer] = useState<HTMLDivElement | null>(null);
  const [inspectorAvailable, setInspectorAvailable] = useState(false);
  const setAvailable = useCallback((available: boolean) => {
    setInspectorAvailable(available);
    onInspectorAvailabilityChange?.(available);
  }, [onInspectorAvailabilityChange]);
  const showAgents = canManageAgents(me);
  const tabs = useMemo(() => TABS.filter((item) =>
    (item.id !== "agents" || showAgents) && (item.id !== "trash" || me.account.kind === "user")
  ), [showAgents, me.account.kind]);
  const current = tabs.find((item) => item.id === tab) ?? tabs[0];
  const bodyScroll = useSettingsScroll(`settings-body-${current.id}`);
  const inspector = useMemo(() => ({
    id: inspectorId, open: inspectorOpen && inspectorAvailable, container,
    onOpen: () => {
      if (container) container.scrollTop = 0;
      onOpenInspector();
    }, setAvailable
  }), [inspectorId, inspectorOpen, inspectorAvailable, container, onOpenInspector, setAvailable]);

  useEffect(() => {
    if (isMobile && !inspectorAvailable && inspectorOpen) onCloseInspector();
  }, [isMobile, inspectorAvailable, inspectorOpen, onCloseInspector]);

  useEffect(() => {
    if (!tabs.some((item) => item.id === tab)) setTab("account");
  }, [tabs, tab]);

  useEffect(() => {
    if (container) container.scrollTop = 0;
  }, [container, current.id]);

  const inspectorContents = <div id={inspectorId} className="ng-settings-inspector-frame flex h-full min-h-0 flex-1 flex-col overflow-hidden">
    <div ref={setContainer} className="ng-settings-inspector min-h-0 flex-1 space-y-5 overflow-y-auto overscroll-contain p-4 text-sm empty:hidden" />
  </div>;

  return (
    <SettingsInspectorContext value={inspector}>
      <div data-settings-view className="ng-settings-view flex min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">
        <WorkbenchSurface title="Settings" tabs={tabs} value={current.id} onChange={setTab}
          navigationLabel="Settings sections" isMobile={isMobile} bodyRef={bodyScroll} bodyClassName="ng-settings-body"
          headerActions={<IconButton label="Close settings" onClick={onClose}><X size={16} /></IconButton>}
          inspector={inspectorAvailable ? {
            title: `${current.label} Inspector`, closeLabel: "Close settings inspector", open: inspectorOpen,
            onClose: onCloseInspector, keepMounted: true, children: inspectorContents
          } : undefined}>
          {current.id === "general" ? <GeneralTab onResetSavedWorkspace={onResetSavedWorkspace} /> : null}
          {current.id === "account" ? <AccountTab me={me} onSignOut={onSignOut} /> : null}
          {current.id === "connections" ? <ConnectionsTab me={me} onSignOut={onSignOut} /> : null}
          {current.id === "agents" ? <AgentsTab canManageAgents={showAgents} /> : null}
          {current.id === "history" ? <EventHistoryPanel spaces={spaces} initialSpaceId={initialSpaceId} canViewAuditEvents={canViewAuditEvents(me)} /> : null}
          {current.id === "trash" ? <TrashPanel /> : null}
        </WorkbenchSurface>
      </div>
    </SettingsInspectorContext>
  );
}
