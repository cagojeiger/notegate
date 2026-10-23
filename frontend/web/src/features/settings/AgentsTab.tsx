import { ArrowLeft, Bot, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { Agent } from "../../api/agents";
import type { Space } from "../../api/types";
import { canWriteSpace } from "../../auth/permissions";
import { AddButton, Button, Card, IconButton, ListItemButton, Notice, SectionHeader, Select, TextField } from "../../shared/ui";
import {
  type AgentSpaceAccessValue, useAgentKeyManagerProps, useAgentsQuery, useConnectionsQuery,
  useCreateAgentMutation, useDeleteAgentMutation, useSetAgentSpaceAccessMutation, useSettingsSpacesQuery
} from "./useSettingsQueries";
import { KeyManager } from "./KeyManager";
import { SettingsControls, SettingsInspectorSection, useSettingsInspector } from "./SettingsInspector";

import { SettingsPage } from "./SettingsPage";
import { useSettingsViewState } from "./settingsViewState";

export function AgentsTab({ canManageAgents }: { canManageAgents: boolean }) {
  const inspectorOpen = useSettingsInspector()?.open ?? true;
  const agentsQuery = useAgentsQuery(canManageAgents);
  const [name, setName] = useState("");
  const [openId, setOpenId] = useSettingsViewState<string | null>("selected-agent", null);
  const [creating, setCreating] = useState(false);
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const createMutation = useCreateAgentMutation((agent) => { setName(""); setCreating(false); setOpenId(agent.id); });
  const deleteMutation = useDeleteAgentMutation();
  useEffect(() => {
    if (deleteMutation.isSuccess) { setConfirmId(null); setOpenId(null); }
  }, [deleteMutation.isSuccess, setOpenId]);
  useEffect(() => {
    if (!inspectorOpen && !deleteMutation.isPending) setConfirmId(null);
  }, [inspectorOpen, deleteMutation.isPending]);
  const agents = agentsQuery.data?.agents ?? [];
  const selected = agents.find((agent) => agent.id === (deleteMutation.isPending ? deleteMutation.variables : openId));
  if (!canManageAgents) return <Notice>Agent management is unavailable for this account.</Notice>;

  return (
    <SettingsPage title={creating ? "New agent" : selected?.name ?? "Agents"} inspectorLabel={selected && !creating ? "Open agent actions" : undefined}
      leading={creating || selected ? <IconButton label="Back to agents" disabled={deleteMutation.isPending || createMutation.isPending}
        onClick={() => { deleteMutation.reset(); setCreating(false); setOpenId(null); setConfirmId(null); }}><ArrowLeft size={16} /></IconButton> : undefined}
      disabled={deleteMutation.isPending || createMutation.isPending} scrollKey={`agent-${creating ? "new" : openId ?? "overview"}`}>
      {deleteMutation.isPending ? <Notice role="status" className="mb-3">Deleting agent…</Notice> : null}
      {deleteMutation.isSuccess ? <Notice tone="success" role="status" className="mb-3">Agent deleted.</Notice> : null}
      {deleteMutation.error ? <Notice tone="danger" role="alert" className="mb-3">{deleteMutation.error.message}</Notice> : null}
      {createMutation.isPending ? <Notice role="status" className="mb-3">Creating agent…</Notice> : null}
      {createMutation.isError ? <Notice tone="danger" role="alert" className="mb-3">Could not create this agent. Try again.</Notice> : null}
      {createMutation.isSuccess && selected?.id === createMutation.data?.id ? <Notice tone="success" role="status" className="mb-3">Agent created.</Notice> : null}
      {!creating && !selected ? <>
        <section>
          <p className="mb-3 text-xs text-muted">Select an agent to manage its permissions and keys.</p>
          {agentsQuery.isLoading ? <Notice role="status">Loading agents…</Notice> : null}
          {agentsQuery.isError ? <Notice tone="danger" role="alert"><p>Could not load agents.</p><Button size="sm" secondary aria-label="Retry agents" disabled={agentsQuery.isFetching} onClick={() => { void agentsQuery.refetch(); }}>Retry</Button></Notice> : null}
          {agentsQuery.isSuccess && agents.length === 0 ? <Notice>No agents yet.</Notice> : null}
          <Card as="ul" aria-label="Agents" padding="none" className="divide-y divide-seam empty:hidden">
            {agents.map((agent) => <li key={agent.id}>
              <ListItemButton aria-label={`Select ${agent.name}`} disabled={deleteMutation.isPending || createMutation.isPending}
                onClick={() => { deleteMutation.reset(); setOpenId(agent.id); setConfirmId(null); }}
                className="items-center">
                <Bot size={15} className="shrink-0 text-muted" /><span className="min-w-0 flex-1 truncate">{agent.name}</span><ChevronRight size={15} className="shrink-0 text-muted" />
              </ListItemButton>
            </li>)}
          </Card>
          <div className="mt-3"><AddButton label="Add agent" disabled={deleteMutation.isPending || createMutation.isPending} onClick={() => { deleteMutation.reset(); setCreating(true); setConfirmId(null); }} /></div>
        </section>
      </> : null}
      <SettingsControls>
        {selected && !creating ? <SettingsInspectorSection title="Agent actions">
          <p className="mb-3 break-all">{selected.name}</p>
          {confirmId === selected.id ? <div className="space-y-3"><p>Delete this agent and revoke its API keys?</p><p className="text-xs text-muted">Its Space connections will be removed. Your browser sessions, CLI and MCP connections stay connected.</p><div className="flex flex-wrap gap-2"><Button size="sm" variant="danger" disabled={deleteMutation.isPending} aria-busy={deleteMutation.isPending} onClick={() => deleteMutation.mutate(selected.id)}>Confirm</Button><Button size="sm" secondary autoFocus disabled={deleteMutation.isPending} onClick={() => { setConfirmId(null); requestAnimationFrame(() => deleteButton.current?.focus()); }}>Cancel</Button></div></div>
            : <Button ref={deleteButton} aria-label={`Delete agent: ${selected.name}`} disabled={deleteMutation.isPending} variant="danger" size="sm" onClick={() => setConfirmId(selected.id)}><Trash2 size={15} /> Delete agent</Button>}
        </SettingsInspectorSection> : null}
      </SettingsControls>
      {creating ? <section>
        <p className="mb-3 text-xs text-muted">Choose a name, then configure permissions and keys.</p>
        <form onSubmit={(event) => { event.preventDefault(); if (name.trim() && !createMutation.isPending) createMutation.mutate(name.trim()); }} className="flex flex-wrap items-end gap-3">
          <TextField autoFocus label="New agent name" className="min-w-0 flex-[1_1_16rem]" value={name} onChange={(event) => setName(event.target.value)} placeholder="e.g. ci-bot, importer" />
          <div className="flex shrink-0 gap-2">
            <Button type="submit" disabled={!name.trim() || createMutation.isPending} aria-busy={createMutation.isPending}><Plus size={15} /> Create</Button>
            <Button secondary disabled={createMutation.isPending} onClick={() => setCreating(false)}>Cancel</Button>
          </div>
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

  if (spacesQuery.isLoading) return <Notice role="status">Loading spaces…</Notice>;
  if (spacesQuery.isError) return (
    <Notice tone="danger" role="alert" className="flex flex-wrap items-center justify-between gap-2">
      <p>Could not load spaces.</p>
      <Button size="sm" secondary aria-label="Retry spaces" disabled={spacesQuery.isFetching} onClick={() => { void spacesQuery.refetch(); }}>Retry</Button>
    </Notice>
  );
  if (spaces.length === 0) return <Notice>No writable spaces.</Notice>;

  return (
    <Card as="ul" padding="none" className="max-h-72 divide-y divide-seam overflow-y-auto">
      {spaces.map((space) => <SpaceAccessRow key={space.id} space={space} agent={agent} />)}
    </Card>
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
    <li className="flex flex-wrap items-center gap-3 px-3 py-3 text-workbench max-sm:flex-col max-sm:items-stretch">
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{space.name}</div>
        {connectionsQuery.isSuccess ? <div className="text-xs text-muted">{ACCESS_LABELS[access]}</div> : null}
      </div>
      {connectionsQuery.isLoading ? (
        <Notice role="status">Loading…</Notice>
      ) : connectionsQuery.isError ? (
        <Notice tone="danger" role="alert" className="flex min-w-0 flex-wrap items-center gap-2">
          <p>Could not load permissions.</p>
          <Button size="sm" secondary aria-label={`Retry ${space.name} permissions`} disabled={connectionsQuery.isFetching} onClick={() => { void connectionsQuery.refetch(); }}>Retry</Button>
        </Notice>
      ) : (
        <div className="w-36 shrink-0 max-sm:w-full"><Select
          aria-label={`${space.name} permission`}
          value={access}
          disabled={setAccessMutation.isPending}
          onChange={(event) => updateAccess(event.currentTarget.value as AgentSpaceAccessValue)}
        >
          <option value="none">{ACCESS_LABELS.none}</option>
          <option value="read">{ACCESS_LABELS.read}</option>
          <option value="write">{ACCESS_LABELS.write}</option>
        </Select></div>
      )}
      {setAccessMutation.isError ? <Notice tone="danger" role="alert" className="basis-full">Could not update access. Check the current permission and try again.</Notice> : null}
      {setAccessMutation.isSuccess ? <Notice tone="success" role="status" className="basis-full">Space access updated.</Notice> : null}
    </li>
  );
}
