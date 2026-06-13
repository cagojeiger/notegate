import { useState } from "react";

import type { Space } from "../api/types";
import { Modal } from "../shared/ui";
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
      <div role="tablist" className="mb-5 flex gap-1 border-b border-seam">
        {TABS.map((t) => (
          <button key={t.id} role="tab" aria-selected={tab === t.id} onClick={() => setTab(t.id)} className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium transition ${tab === t.id ? "border-primary text-text" : "border-transparent text-muted hover:text-text"}`}>{t.label}</button>
        ))}
      </div>
      {tab === "account" ? <AccountTab onSignOut={onSignOut} /> : null}
      {tab === "keys" ? <UserKeysTab /> : null}
      {tab === "agents" ? <AgentsTab /> : null}
      {tab === "connections" ? <ConnectionsTab activeSpace={activeSpace} /> : null}
    </Modal>
  );
}
