import type { Me } from "../entities/account/model";
import type { Space } from "../entities/space/model";

const DEFAULT_CAPABILITIES: Me["capabilities"] = {
  can_create_space: false,
  can_manage_agents: false
};

export function capabilitiesFor(me: Me | null | undefined): Me["capabilities"] {
  return me?.capabilities ?? DEFAULT_CAPABILITIES;
}

export function canCreateSpace(me: Me | null | undefined): boolean {
  return capabilitiesFor(me).can_create_space;
}

export function canManageAgents(me: Me | null | undefined): boolean {
  return capabilitiesFor(me).can_manage_agents;
}

export function canViewAuditEvents(me: Me | null | undefined): boolean {
  return me?.account.kind === "user";
}

export function canWriteSpace(space: Space | null | undefined): boolean {
  return space?.permission === "write";
}

export function canManageSpace(me: Me | null | undefined, space: Space | null | undefined): boolean {
  return canCreateSpace(me) && canWriteSpace(space);
}
