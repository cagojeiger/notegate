import type { RestNode } from "../api/types";
import { MAX_EDITOR_GROUPS, resetEditorGroupsState, type EditorGroup, type EditorGroupState } from "./uiStoreReducers";

const WORKBENCH_VERSION = 1;
const WORKBENCH_INDEX_KEY = "notegate.workbench.v1.index";
const WORKBENCH_SPACE_KEY_PREFIX = "notegate.workbench.v1.space.";
const WORKBENCH_PANEL_STATE_KEY = "notegate.workbenchPanels.v1";
const LAST_ACTIVE_SPACE_KEY = "notegate.lastActiveSpaceId";
const MAX_WORKBENCH_SNAPSHOTS = 20;

type PersistedEditorGroup = {
  node: RestNode | null;
  mode: "preview" | "edit";
};

type PersistedSpaceWorkbench = {
  version: 1;
  spaceId: string;
  updatedAt: number;
  activeGroupIndex: number;
  groups: PersistedEditorGroup[];
};

type WorkbenchIndex = {
  version: 1;
  spaces: { spaceId: string; updatedAt: number }[];
};

type WorkbenchPanelState = {
  primarySidebarOpen: boolean;
  auxiliaryOpen: boolean;
};

type PersistedWorkbenchPanelState = WorkbenchPanelState & {
  version: 1;
};

const DEFAULT_WORKBENCH_PANEL_STATE: WorkbenchPanelState = {
  primarySidebarOpen: true,
  auxiliaryOpen: true
};

export { LAST_ACTIVE_SPACE_KEY, MAX_WORKBENCH_SNAPSHOTS, WORKBENCH_INDEX_KEY, WORKBENCH_PANEL_STATE_KEY };

export function readLastActiveSpace(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(LAST_ACTIVE_SPACE_KEY);
}

export function persistLastActiveSpace(spaceId: string): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(LAST_ACTIVE_SPACE_KEY, spaceId);
}

export function workbenchSpaceKey(spaceId: string): string {
  return `${WORKBENCH_SPACE_KEY_PREFIX}${spaceId}`;
}

export function restoreSpaceWorkbench(spaceId: string, nextGroupId: number): EditorGroupState {
  const saved = readSpaceWorkbench(spaceId);
  if (!saved) return emptyEditorGroupState(nextGroupId);

  const editorGroups = saved.groups.slice(0, MAX_EDITOR_GROUPS).map((group, index) => {
    const savedGroup = group && typeof group === "object" ? group as Partial<PersistedEditorGroup> : {};
    const node = isRestNodeForSpace(savedGroup.node, spaceId) ? savedGroup.node : null;
    return {
      id: nextGroupId + index,
      node,
      mode: node && savedGroup.mode === "edit" ? "edit" as const : "preview" as const
    };
  });

  if (editorGroups.length === 0) return emptyEditorGroupState(nextGroupId);
  return {
    editorGroups,
    activeGroupIndex: clampIndex(saved.activeGroupIndex, editorGroups.length),
    nextGroupId: nextGroupId + editorGroups.length
  };
}

export function persistSpaceWorkbench(spaceId: string, editorGroups: EditorGroup[], activeGroupIndex: number): void {
  if (typeof window === "undefined") return;
  const updatedAt = Date.now();
  const groups = editorGroups.slice(0, MAX_EDITOR_GROUPS).map((group) => ({
    node: group.node?.space_id === spaceId ? group.node : null,
    mode: group.node && group.mode === "edit" ? "edit" as const : "preview" as const
  }));
  const snapshot: PersistedSpaceWorkbench = {
    version: WORKBENCH_VERSION,
    spaceId,
    updatedAt,
    activeGroupIndex: clampIndex(activeGroupIndex, groups.length),
    groups
  };

  try {
    window.localStorage.setItem(workbenchSpaceKey(spaceId), JSON.stringify(snapshot));
    updateWorkbenchIndex(spaceId, updatedAt);
  } catch {
    // Browser storage can be unavailable or full. Restoring panes is best-effort.
  }
}

export function restoreWorkbenchPanelState(): WorkbenchPanelState {
  if (typeof window === "undefined") return DEFAULT_WORKBENCH_PANEL_STATE;
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(WORKBENCH_PANEL_STATE_KEY) ?? "null");
    if (!isPersistedWorkbenchPanelState(parsed)) return DEFAULT_WORKBENCH_PANEL_STATE;
    return {
      primarySidebarOpen: parsed.primarySidebarOpen,
      auxiliaryOpen: parsed.auxiliaryOpen
    };
  } catch {
    return DEFAULT_WORKBENCH_PANEL_STATE;
  }
}

export function persistWorkbenchPanelState(state: WorkbenchPanelState): void {
  if (typeof window === "undefined") return;
  try {
    const snapshot: PersistedWorkbenchPanelState = { version: WORKBENCH_VERSION, ...state };
    window.localStorage.setItem(WORKBENCH_PANEL_STATE_KEY, JSON.stringify(snapshot));
  } catch {
    // Browser storage can be unavailable or full. Panel visibility is best-effort.
  }
}

export function clearPersistedSpaceWorkbench(spaceId: string): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(workbenchSpaceKey(spaceId));
    writeIndex({ version: WORKBENCH_VERSION, spaces: readIndex().spaces.filter((item) => item.spaceId !== spaceId) });
  } catch {
    // Browser storage cleanup is best-effort.
  }
}

export function clearPersistedWorkbenches(): void {
  if (typeof window === "undefined") return;
  try {
    for (let i = window.localStorage.length - 1; i >= 0; i -= 1) {
      const key = window.localStorage.key(i);
      if (key?.startsWith(WORKBENCH_SPACE_KEY_PREFIX)) window.localStorage.removeItem(key);
    }
    window.localStorage.removeItem(WORKBENCH_INDEX_KEY);
    window.localStorage.removeItem(WORKBENCH_PANEL_STATE_KEY);
    window.localStorage.removeItem(LAST_ACTIVE_SPACE_KEY);
  } catch {
    // Browser storage cleanup is best-effort.
  }
}

function emptyEditorGroupState(nextGroupId: number): EditorGroupState {
  return resetEditorGroupsState({ nextGroupId });
}

function readSpaceWorkbench(spaceId: string): PersistedSpaceWorkbench | null {
  if (typeof window === "undefined") return null;
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(workbenchSpaceKey(spaceId)) ?? "null");
    if (!isPersistedSpaceWorkbench(parsed, spaceId)) {
      window.localStorage.removeItem(workbenchSpaceKey(spaceId));
      return null;
    }
    return parsed;
  } catch {
    window.localStorage.removeItem(workbenchSpaceKey(spaceId));
    return null;
  }
}

function updateWorkbenchIndex(spaceId: string, updatedAt: number): void {
  const indexed = readIndex().spaces.filter((item) => item.spaceId !== spaceId);
  indexed.push({ spaceId, updatedAt });
  indexed.sort((a, b) => b.updatedAt - a.updatedAt);

  for (const item of indexed.slice(MAX_WORKBENCH_SNAPSHOTS)) {
    window.localStorage.removeItem(workbenchSpaceKey(item.spaceId));
  }
  writeIndex({ version: WORKBENCH_VERSION, spaces: indexed.slice(0, MAX_WORKBENCH_SNAPSHOTS) });
}

function readIndex(): WorkbenchIndex {
  if (typeof window === "undefined") return { version: WORKBENCH_VERSION, spaces: [] };
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(WORKBENCH_INDEX_KEY) ?? "null");
    if (!isWorkbenchIndex(parsed)) return { version: WORKBENCH_VERSION, spaces: [] };
    return { version: WORKBENCH_VERSION, spaces: parsed.spaces.filter(isWorkbenchIndexEntry) };
  } catch {
    return { version: WORKBENCH_VERSION, spaces: [] };
  }
}

function writeIndex(index: WorkbenchIndex): void {
  try {
    window.localStorage.setItem(WORKBENCH_INDEX_KEY, JSON.stringify(index));
  } catch {
    // Browser storage can be unavailable or full. Restoring panes is best-effort.
  }
}

function isWorkbenchIndex(value: unknown): value is WorkbenchIndex {
  if (!value || typeof value !== "object") return false;
  const index = value as Partial<WorkbenchIndex>;
  return index.version === WORKBENCH_VERSION && Array.isArray(index.spaces);
}

function isWorkbenchIndexEntry(value: unknown): value is WorkbenchIndex["spaces"][number] {
  if (!value || typeof value !== "object") return false;
  const entry = value as Partial<WorkbenchIndex["spaces"][number]>;
  return typeof entry.spaceId === "string" && Number.isFinite(entry.updatedAt);
}

function isPersistedSpaceWorkbench(value: unknown, spaceId: string): value is PersistedSpaceWorkbench {
  if (!value || typeof value !== "object") return false;
  const snapshot = value as Partial<PersistedSpaceWorkbench>;
  return snapshot.version === WORKBENCH_VERSION && snapshot.spaceId === spaceId && Number.isFinite(snapshot.updatedAt) && Number.isInteger(snapshot.activeGroupIndex) && Array.isArray(snapshot.groups);
}

function isPersistedWorkbenchPanelState(value: unknown): value is PersistedWorkbenchPanelState {
  if (!value || typeof value !== "object") return false;
  const state = value as Partial<PersistedWorkbenchPanelState>;
  return state.version === WORKBENCH_VERSION && typeof state.primarySidebarOpen === "boolean" && typeof state.auxiliaryOpen === "boolean";
}

function isRestNodeForSpace(value: unknown, spaceId: string): value is RestNode {
  if (!value || typeof value !== "object") return false;
  const node = value as Partial<RestNode>;
  return (
    node.space_id === spaceId &&
    typeof node.id === "string" &&
    typeof node.name === "string" &&
    typeof node.path === "string" &&
    (node.parent_id === null || typeof node.parent_id === "string") &&
    (node.kind === "folder" || node.kind === "text" || node.kind === "file") &&
    typeof node.sort_order === "number" &&
    Boolean(node.metadata) &&
    typeof node.metadata === "object" &&
    !Array.isArray(node.metadata) &&
    typeof node.has_children === "boolean" &&
    isAccountRef(node.created_by) &&
    isAccountRef(node.updated_by) &&
    typeof node.created_at === "string" &&
    typeof node.updated_at === "string"
  );
}

function isAccountRef(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  const account = value as Partial<RestNode["created_by"]>;
  return typeof account.id === "string" && (account.kind === "user" || account.kind === "agent") && typeof account.display_name === "string";
}

function clampIndex(index: number, length: number): number {
  return Math.max(0, Math.min(Number.isInteger(index) ? index : 0, Math.max(0, length - 1)));
}
