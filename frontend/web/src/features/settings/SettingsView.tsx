import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { X } from "lucide-react";

import type { Me, Space } from "../../api/types";
import { canManageAgents, canViewAuditEvents } from "../../auth/permissions";
import { WORKBENCH_LAYOUT } from "../../shared/model/workbenchLayout";
import { IconButton, Modal, PanelHeader, Tabs } from "../../shared/ui";
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
};

export function SettingsView(props: SettingsViewProps) {
  return <SettingsViewSession><SettingsWorkbench {...props} /></SettingsViewSession>;
}

function SettingsWorkbench({ me, spaces = [], initialSpaceId = null, onClose, onSignOut,
  onResetSavedWorkspace = () => undefined, isMobile = false, inspectorOpen = true,
  onOpenInspector = () => undefined, onCloseInspector = () => undefined
}: SettingsViewProps) {
  const [tab, setTab] = useState<Tab>("general");
  const inspectorId = useId();
  const [container, setContainer] = useState<HTMLDivElement | null>(null);
  const [navigationContainer, setNavigationContainer] = useState<HTMLDivElement | null>(null);
  const showAgents = canManageAgents(me);
  const tabs = useMemo(() => TABS.filter((item) =>
    (item.id !== "agents" || showAgents) && (item.id !== "trash" || me.account.kind === "user")
  ), [showAgents, me.account.kind]);
  const current = tabs.find((item) => item.id === tab) ?? tabs[0];
  const inspector = useMemo(() => ({ id: inspectorId, open: inspectorOpen, container, navigationContainer, onOpen: onOpenInspector, onNavigate: () => { if (isMobile) onCloseInspector(); } }), [inspectorId, inspectorOpen, container, navigationContainer, onOpenInspector, isMobile, onCloseInspector]);

  useEffect(() => {
    if (!tabs.some((item) => item.id === tab)) setTab("account");
  }, [tabs, tab]);

  useEffect(() => {
    if (container) container.scrollTop = 0;
  }, [container, current.id]);

  const inspectorContents = <div id={inspectorId} className="ng-settings-inspector-frame flex h-full min-h-0 flex-1 flex-col overflow-hidden">
    <div ref={setNavigationContainer} className="ng-settings-navigation max-h-[60%] shrink-0 overflow-y-auto overscroll-contain border-b border-seam p-4 text-sm empty:hidden" />
    <div ref={setContainer} className="ng-settings-inspector min-h-0 flex-1 space-y-5 overflow-y-auto overscroll-contain p-4 text-sm empty:hidden" />
  </div>;

  return (
    <SettingsInspectorContext value={inspector}>
      <div data-settings-view className="ng-settings-view flex min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">
        <section aria-label="Settings" className="flex min-h-0 min-w-0 flex-1 flex-col">
          <h1 className="sr-only">Settings</h1>
          <PanelHeader tabs>
            <Tabs items={tabs} value={current.id} onChange={setTab} label="Settings sections" variant="header" />
            <div className="flex h-full shrink-0 items-center"><IconButton label="Close settings" onClick={onClose}><X size={16} /></IconButton></div>
          </PanelHeader>
          <SettingsBody key={current.id} tab={current}>
            {current.id === "general" ? <GeneralTab onResetSavedWorkspace={onResetSavedWorkspace} /> : null}
            {current.id === "account" ? <AccountTab me={me} onSignOut={onSignOut} /> : null}
            {current.id === "connections" ? <ConnectionsTab me={me} onSignOut={onSignOut} /> : null}
            {current.id === "agents" ? <AgentsTab canManageAgents={showAgents} /> : null}
            {current.id === "history" ? <EventHistoryPanel spaces={spaces} initialSpaceId={initialSpaceId} canViewAuditEvents={canViewAuditEvents(me)} /> : null}
            {current.id === "trash" ? <TrashPanel /> : null}
          </SettingsBody>
        </section>
        {!isMobile ? (
          <aside aria-label={`${current.label} Inspector`} className={inspectorOpen ? "flex min-h-0 shrink-0 flex-col border-l border-seam bg-surface" : "hidden"} style={{ width: WORKBENCH_LAYOUT.defaultAuxiliaryWidth }}>
            <PanelHeader><h2 className="font-medium">{current.label} Inspector</h2><IconButton label="Close settings inspector" onClick={onCloseInspector}><X size={16} /></IconButton></PanelHeader>
            {inspectorContents}
          </aside>
        ) : inspectorOpen ? (
          <Modal title={`${current.label} Inspector`} placement="bottom" width="max-w-none" onClose={onCloseInspector}>{inspectorContents}</Modal>
        ) : null}
      </div>
    </SettingsInspectorContext>
  );
}

function SettingsBody({ tab, children }: { tab: typeof TABS[number]; children: ReactNode }) {
  const scroll = useSettingsScroll(`settings-body-${tab.id}`);
  return <div ref={scroll} id={tab.controls} role="tabpanel" aria-labelledby={`${tab.controls}-tab`}
    className="ng-settings-body min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain p-4 md:p-5">{children}</div>;
}
