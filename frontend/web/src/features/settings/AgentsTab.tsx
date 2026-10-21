import { ArrowLeft, Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useRef, useState } from "react";

import type { Agent } from "../../api/agents";
import type { Space } from "../../api/types";
import { canWriteSpace } from "../../auth/permissions";
import { Button, Card, EmptyState, IconButton, SectionHeader, TextField } from "../../shared/ui";
import {
  type AgentSpaceAccessValue, useAgentKeyManagerProps, useAgentsQuery, useConnectionsQuery,
  useCreateAgentMutation, useDeleteAgentMutation, useSetAgentSpaceAccessMutation, useSettingsSpacesQuery
} from "./useSettingsQueries";
import { KeyManager } from "./KeyManager";
import { SettingsControls, SettingsInspectorSection } from "./SettingsInspector";

import { SettingsPage } from "./SettingsPage";
import { SettingsAddButton } from "./SettingsAddButton";
import { useSettingsViewState } from "./settingsViewState";

export function AgentsTab({ canManageAgents }: { canManageAgents: boolean }) {
  const agentsQuery = useAgentsQuery(canManageAgents);
  const [name, setName] = useState("");
  const [openId, setOpenId] = useSettingsViewState<string | null>("selected-agent", null);
  const [creating, setCreating] = useState(false);
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const createMutation = useCreateAgentMutation((agent) => { setName(""); setCreating(false); setOpenId(agent.id); });
  const deleteMutation = useDeleteAgentMutation(() => { setConfirmId(null); setOpenId(null); });
  const agents = agentsQuery.data?.agents ?? [];
  const selected = agents.find((agent) => agent.id === openId);
  if (!canManageAgents) return <EmptyState>Agent management is unavailable for this account.</EmptyState>;

  return (
    <SettingsPage title={creating ? "New agent" : selected?.name ?? "Agents"} inspectorLabel={selected && !creating ? "Open agent actions" : undefined}
      leading={creating || selected ? <IconButton label="Back to agents" disabled={deleteMutation.isPending || createMutation.isPending}
        onClick={() => { setCreating(false); setOpenId(null); setConfirmId(null); }}><ArrowLeft size={16} /></IconButton> : undefined}
      disabled={deleteMutation.isPending || createMutation.isPending} scrollKey={`agent-${creating ? "new" : openId ?? "overview"}`}>
      {!creating && !selected ? <>
        <section>
          <p className="mb-3 text-xs text-muted">Select an agent to manage its permissions and keys.</p>
          {agentsQuery.isLoading ? <p role="status" className="text-muted">Loading agents…</p> : null}
          {agentsQuery.isError ? <div><p role="alert" className="text-danger">Could not load agents.</p><Button size="sm" secondary aria-label="Retry agents" disabled={agentsQuery.isFetching} onClick={() => { void agentsQuery.refetch(); }}>Retry</Button></div> : null}
          {agentsQuery.isSuccess && agents.length === 0 ? <p className="text-muted">No agents yet.</p> : null}
          <Card as="ul" aria-label="Agents" padding="none" className="divide-y divide-seam empty:hidden">
            {agents.map((agent) => <li key={agent.id}>
              <button type="button" aria-label={`Select ${agent.name}`} disabled={deleteMutation.isPending || createMutation.isPending}
                onClick={() => { setOpenId(agent.id); setConfirmId(null); }}
                className="flex min-h-[var(--ng-min-target-size)] w-full items-center gap-3 px-4 py-3 text-left text-workbench hover:bg-[var(--ng-hover)]">
                <Bot size={15} className="shrink-0 text-muted" /><span className="min-w-0 flex-1 truncate">{agent.name}</span><ChevronRight size={15} className="shrink-0 text-muted" />
              </button>
            </li>)}
          </Card>
          <div className="mt-3"><SettingsAddButton disabled={deleteMutation.isPending || createMutation.isPending} onClick={() => { setCreating(true); setConfirmId(null); }}>Add agent</SettingsAddButton></div>
        </section>
      </> : null}
      <SettingsControls>
        {selected && !creating ? <SettingsInspectorSection title="Agent actions">
          {confirmId === selected.id ? <div className="space-y-3"><p>Delete this agent and revoke its API keys?</p><div className="flex flex-wrap gap-2"><Button size="sm" variant="danger" disabled={deleteMutation.isPending} onClick={() => deleteMutation.mutate(selected.id)}>Confirm</Button><Button size="sm" secondary autoFocus disabled={deleteMutation.isPending} onClick={() => { setConfirmId(null); requestAnimationFrame(() => deleteButton.current?.focus()); }}>Cancel</Button></div></div>
            : <Button ref={deleteButton} variant="danger" size="sm" onClick={() => setConfirmId(selected.id)}><Trash2 size={15} /> Delete {selected.name}</Button>}
        </SettingsInspectorSection> : null}
      </SettingsControls>
      {creating ? <section>
        <p className="mb-3 text-xs text-muted">Choose a name, then configure permissions and keys.</p>
        <form onSubmit={(event) => { event.preventDefault(); if (name.trim() && !createMutation.isPending) createMutation.mutate(name.trim()); }} className="flex flex-wrap items-end gap-3">
          <TextField autoFocus label="New agent name" className="min-w-0 flex-1" value={name} onChange={(event) => setName(event.target.value)} placeholder="e.g. ci-bot, importer" />
          <Button type="submit" disabled={!name.trim() || createMutation.isPending}><Plus size={15} /> Create</Button>
          <Button secondary disabled={createMutation.isPending} onClick={() => setCreating(false)}>Cancel</Button>
        </form>
      </section> : selected ? <section key={selected.id} aria-label={`Settings for ${selected.name}`}>
        <div className="ng-settings-grid">
          <section><SectionHeader title="Space permissions" description="Choose which spaces this agent can access." /><AgentSpaceAccess agent={selected} /></section>
          <section><SectionHeader title="Agent API Keys" description="Keys authenticate as this agent." /><AgentKeyManager agentId={selected.id} /></section>
        </div>
      </section> : null}
    </SettingsPage>
  );
}

function AgentKeyManager({ agentId }: { agentId: string }) {
  const keyManagerProps = useAgentKeyManagerProps(agentId);
  return <KeyManager {...keyManagerProps} emptyLabel="No keys for this agent." maxTtlDays={365} />;
}

function AgentSpaceAccess({ agent }: { agent: Agent }) {
  const spacesQuery = useSettingsSpacesQuery(true);
  const spaces = (spacesQuery.data?.spaces ?? []).filter(canWriteSpace);

  if (spacesQuery.isLoading) return <div className="text-sm text-muted">Loading spaces…</div>;
  if (spacesQuery.isError) return (
    <Card tone="danger" padding="sm" className="flex flex-wrap items-center justify-between gap-2 text-workbench">
      <p role="alert" className="text-danger">Could not load spaces.</p>
      <Button size="sm" secondary aria-label="Retry spaces" disabled={spacesQuery.isFetching} onClick={() => { void spacesQuery.refetch(); }}>Retry</Button>
    </Card>
  );
  if (spaces.length === 0) return <EmptyState>No writable spaces.</EmptyState>;

  return (
    <ul className="max-h-72 divide-y divide-seam overflow-y-auto rounded-workbench-surface border border-border bg-surface">
      {spaces.map((space) => <SpaceAccessRow key={space.id} space={space} agent={agent} />)}
    </ul>
  );
}

const ACCESS_LABELS: Record<AgentSpaceAccessValue, string> = {
  none: "No access",
  read: "Read only",
  write: "Read & write"
};

function SpaceAccessRow({ space, agent }: { space: Space; agent: Agent }) {
  const connectionsQuery = useConnectionsQuery(space.id);
  const setAccessMutation = useSetAgentSpaceAccessMutation();
  const connection = connectionsQuery.data?.connections.find((item) => item.agent.id === agent.id);
  const access: AgentSpaceAccessValue = connection?.permission ?? "none";

  const updateAccess = (nextAccess: AgentSpaceAccessValue) => {
    if (nextAccess === access) return;
    if (nextAccess === "none" && !connection) return;
    setAccessMutation.mutate({ spaceId: space.id, agent, access: nextAccess });
  };

  return (
    <li className="flex items-center gap-3 px-4 py-3 text-sm max-sm:flex-col max-sm:items-stretch">
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{space.name}</div>
        {connectionsQuery.isSuccess ? <div className="text-xs text-muted">{ACCESS_LABELS[access]}</div> : null}
      </div>
      {connectionsQuery.isLoading ? (
        <span className="text-xs text-muted">Loading…</span>
      ) : connectionsQuery.isError ? (
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <p role="alert" className="text-workbench text-danger">Could not load permissions.</p>
          <Button size="sm" secondary aria-label={`Retry ${space.name} permissions`} disabled={connectionsQuery.isFetching} onClick={() => { void connectionsQuery.refetch(); }}>Retry</Button>
        </div>
      ) : (
        <select
          aria-label={`${space.name} permission`}
          className="h-9 min-h-[var(--ng-min-target-size)] w-36 shrink-0 rounded-workbench-surface border border-border-strong bg-surface px-3 text-sm text-text outline-none transition disabled:cursor-not-allowed disabled:opacity-50 max-sm:w-full"
          value={access}
          disabled={setAccessMutation.isPending}
          onChange={(event) => updateAccess(event.currentTarget.value as AgentSpaceAccessValue)}
        >
          <option value="none">{ACCESS_LABELS.none}</option>
          <option value="read">{ACCESS_LABELS.read}</option>
          <option value="write">{ACCESS_LABELS.write}</option>
        </select>
      )}
    </li>
  );
}
