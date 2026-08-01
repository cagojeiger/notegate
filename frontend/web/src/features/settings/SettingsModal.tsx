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
    <Modal title="Settings" onClose={onClose} width="max-w-2xl">
      <Tabs items={tabs} value={tab} onChange={setTab} label="Settings sections" />
      <div className="min-h-[34rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1">
        {tab === "general" ? <GeneralTab onResetSavedWorkspace={onResetSavedWorkspace} /> : null}
        {tab === "account" ? <AccountTab me={me} onSignOut={onSignOut} /> : null}
        {tab === "agents" && showAgents ? <AgentsTab canManageAgents={showAgents} /> : null}
      </div>
    </Modal>
  );
}
