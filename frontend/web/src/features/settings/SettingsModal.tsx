import { useEffect, useMemo, useState } from "react";

import type { Me } from "../../api/types";
import { canManageAgents } from "../../auth/permissions";
import { Modal, Tabs } from "../../shared/ui";
import { AccountTab } from "./AccountTab";
import { AgentsTab } from "./AgentsTab";
import { GeneralTab } from "./GeneralTab";

type Tab = "general" | "account" | "agents";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "account", label: "Account" },
  { id: "agents", label: "Agents" }
];

export function SettingsModal({ me, onClose, onSignOut, onResetSavedWorkspace = () => undefined }: { me: Me; onClose: () => void; onSignOut: () => void; onResetSavedWorkspace?: () => void }) {
  const [tab, setTab] = useState<Tab>("general");
  const showAgents = canManageAgents(me);
  const tabs = useMemo(
    () => TABS.filter((item) => item.id !== "agents" || showAgents),
    [showAgents]
  );

  useEffect(() => {
    if (tab === "agents" && !showAgents) setTab("account");
  }, [showAgents, tab]);

  return (
    <Modal title="Settings" onClose={onClose} width="max-w-2xl" fixedHeight>
      <div className="shrink-0"><Tabs items={tabs} value={tab} onChange={setTab} label="Settings sections" /></div>
      <div key={tab} role="tabpanel" aria-label={`${tabs.find((item) => item.id === tab)?.label ?? "General"} settings`} className="min-h-0 flex-1 overflow-y-auto overscroll-contain pr-1 [scrollbar-gutter:stable]">
        {tab === "general" ? <GeneralTab onResetSavedWorkspace={onResetSavedWorkspace} /> : null}
        {tab === "account" ? <AccountTab me={me} onSignOut={onSignOut} /> : null}
        {tab === "agents" && showAgents ? <AgentsTab canManageAgents={showAgents} /> : null}
      </div>
    </Modal>
  );
}
