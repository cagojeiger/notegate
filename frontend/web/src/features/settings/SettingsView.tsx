import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";

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
        <section aria-label="Settings" className="flex min-h-0 min-w-0 flex-1 flex-col">
          <h1 className="sr-only">Settings</h1>
          <PanelHeader tabs>
            <SettingsSectionTabs items={tabs} value={current.id} onChange={setTab} />
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
        {inspectorAvailable && !isMobile ? (
          <aside aria-label={`${current.label} Inspector`} className={inspectorOpen ? "flex min-h-0 shrink-0 flex-col border-l border-seam bg-surface" : "hidden"} style={{ width: WORKBENCH_LAYOUT.defaultAuxiliaryWidth }}>
            <PanelHeader><h2 className="font-medium">{current.label} Inspector</h2><IconButton label="Close settings inspector" onClick={onCloseInspector}><X size={16} /></IconButton></PanelHeader>
            {inspectorContents}
          </aside>
        ) : inspectorAvailable && inspectorOpen ? (
          <Modal title={`${current.label} Inspector`} placement="bottom" width="max-w-none" onClose={onCloseInspector}>{inspectorContents}</Modal>
        ) : null}
      </div>
    </SettingsInspectorContext>
  );
}

function SettingsSectionTabs({ items, value, onChange }: { items: typeof TABS; value: Tab; onChange: (tab: Tab) => void }) {
  const frame = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ overflow: false, before: false, after: false });
  const revealSelected = useCallback(() => {
    const list = frame.current?.querySelector<HTMLDivElement>('[role="tablist"]');
    const active = list?.querySelector<HTMLButtonElement>('[aria-selected="true"]');
    if (!list || !active) return;
    const visible = list.getBoundingClientRect();
    const selected = active.getBoundingClientRect();
    if (selected.left < visible.left) list.scrollLeft -= Math.ceil(visible.left - selected.left);
    else if (selected.right > visible.right) list.scrollLeft += Math.ceil(selected.right - visible.right);
  }, []);
  useLayoutEffect(() => {
    const outer = frame.current!;
    const list = outer.querySelector<HTMLDivElement>('[role="tablist"]')!;
    const update = () => {
      const next = { overflow: list.scrollWidth > outer.clientWidth + 1, before: list.scrollLeft > 1,
        after: list.scrollWidth - list.clientWidth - list.scrollLeft > 1 };
      setEdges((current) => current.overflow === next.overflow && current.before === next.before && current.after === next.after ? current : next);
    };
    update();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => { revealSelected(); update(); });
    observer?.observe(outer);
    observer?.observe(list);
    list.addEventListener("scroll", update);
    return () => { observer?.disconnect(); list.removeEventListener("scroll", update); };
  }, [items, revealSelected]);
  useLayoutEffect(() => { revealSelected(); }, [value, edges.overflow, revealSelected]);
  const scroll = (direction: number) => {
    const list = frame.current?.querySelector<HTMLDivElement>('[role="tablist"]');
    if (list) list.scrollLeft += direction * list.clientWidth * 0.75;
  };
  return <div ref={frame} className="flex h-full min-w-0 flex-1 items-center">
    {edges.overflow ? <IconButton label="Previous settings sections" disabled={!edges.before} onClick={() => scroll(-1)}><ChevronLeft size={16} /></IconButton> : null}
    <div className="h-full min-w-0 flex-1"><Tabs items={items} value={value} onChange={onChange} label="Settings sections" variant="header" /></div>
    {edges.overflow ? <IconButton label="More settings sections" disabled={!edges.after} onClick={() => scroll(1)}><ChevronRight size={16} /></IconButton> : null}
  </div>;
}

function SettingsBody({ tab, children }: { tab: typeof TABS[number]; children: ReactNode }) {
  const scroll = useSettingsScroll(`settings-body-${tab.id}`);
  return <div ref={scroll} id={tab.controls} role="tabpanel" aria-labelledby={`${tab.controls}-tab`}
    className="ng-settings-body min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain p-4 md:p-5">{children}</div>;
}
