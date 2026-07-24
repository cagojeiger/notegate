import { useEffect, useMemo, useState } from "react";

import type { Me } from "../entities/account/model";
import { canManageAgents } from "../auth/permissions";
import { Modal, Tabs } from "../shared/ui";
import { AccountTab } from "./settings/AccountTab";
import { AgentsTab } from "./settings/AgentsTab";
import { GeneralTab } from "./settings/GeneralTab";
import { McpTab } from "./settings/McpTab";
import { UsageTab } from "./settings/UsageTab";

type Tab = "general" | "account" | "usage" | "agents" | "mcp";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "account", label: "Account" },
  { id: "usage", label: "Usage" },
  { id: "agents", label: "Agents" },
  { id: "mcp", label: "MCP" }
];

export function SettingsModal({ me, onClose, onSignOut, onResetSavedWorkspace = () => undefined }: { me: Me; onClose: () => void; onSignOut: () => void; onResetSavedWorkspace?: () => void }) {
  const [tab, setTab] = useState<Tab>("general");
  const showAgents = canManageAgents(me);
  const showUsage = me.account.kind === "user";
  const tabs = useMemo(
    () => TABS.filter((item) => (item.id !== "agents" || showAgents) && (item.id !== "usage" || showUsage)),
    [showAgents, showUsage]
  );

  useEffect(() => {
    if (tab === "agents" && !showAgents) setTab("account");
    if (tab === "usage" && !showUsage) setTab("account");
  }, [showAgents, showUsage, tab]);

  return (
    <Modal title="Settings" onClose={onClose} width="max-w-2xl">
      <Tabs items={tabs} value={tab} onChange={setTab} label="Settings sections" />
      <div className="min-h-[34rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1">
        {tab === "general" ? <GeneralTab onResetSavedWorkspace={onResetSavedWorkspace} /> : null}
        {tab === "account" ? <AccountTab me={me} onSignOut={onSignOut} /> : null}
        {tab === "usage" && showUsage ? <UsageTab /> : null}
        {tab === "agents" && showAgents ? <AgentsTab canManageAgents={showAgents} /> : null}
        {tab === "mcp" ? <McpTab /> : null}
      </div>
    </Modal>
  );
}
