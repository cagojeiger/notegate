import { useState } from "react";

import { Modal, Tabs } from "../shared/ui";
import { AccountTab } from "./settings/AccountTab";
import { AgentsTab } from "./settings/AgentsTab";

type Tab = "account" | "agents";

const TABS: { id: Tab; label: string }[] = [
  { id: "account", label: "Account" },
  { id: "agents", label: "Agents" }
];

export function SettingsModal({ onClose, onSignOut }: { onClose: () => void; onSignOut: () => void }) {
  const [tab, setTab] = useState<Tab>("account");
  return (
    <Modal title="Settings" onClose={onClose} width="max-w-2xl">
      <Tabs items={TABS} value={tab} onChange={setTab} label="Settings sections" />
      <div className="min-h-[34rem] max-h-[min(68vh,42rem)] overflow-y-auto pr-1">
        {tab === "account" ? <AccountTab onSignOut={onSignOut} /> : null}
        {tab === "agents" ? <AgentsTab /> : null}
      </div>
    </Modal>
  );
}
