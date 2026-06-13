import { useState } from "react";

import type { Space } from "../api/types";
import { Modal, Tabs } from "../shared/ui";
import { AccountTab } from "./settings/AccountTab";
import { AgentsTab } from "./settings/AgentsTab";
import { ConnectionsTab } from "./settings/ConnectionsTab";
import { UserKeysTab } from "./settings/UserKeysTab";

type Tab = "account" | "keys" | "agents" | "connections";

const TABS: { id: Tab; label: string }[] = [
  { id: "account", label: "Account" },
  { id: "keys", label: "API Keys" },
  { id: "agents", label: "Agents" },
  { id: "connections", label: "Connections" }
];

export function SettingsModal({ onClose, onSignOut, activeSpace }: { onClose: () => void; onSignOut: () => void; activeSpace: Space | null }) {
  const [tab, setTab] = useState<Tab>("account");
  return (
    <Modal title="Settings" onClose={onClose} width="max-w-2xl">
      <Tabs items={TABS} value={tab} onChange={setTab} label="Settings sections" />
      {tab === "account" ? <AccountTab onSignOut={onSignOut} /> : null}
      {tab === "keys" ? <UserKeysTab /> : null}
      {tab === "agents" ? <AgentsTab /> : null}
      {tab === "connections" ? <ConnectionsTab activeSpace={activeSpace} /> : null}
    </Modal>
  );
}
