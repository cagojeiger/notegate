import type { NodeKind, RestNode } from "../api/types";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";

export const MAX_EDITOR_NAVIGATION_ENTRIES = 50;

export type EditorNavigationDirection = "back" | "forward";

export type EditorNavigationEntry = {
  spaceId: string;
  nodeId: string;
  nameSnapshot: string;
  kind: NodeKind;
};

export type EditorGroup = {
  id: number;
  node: RestNode | null;
  mode: "preview" | "edit";
  back: EditorNavigationEntry[];
  forward: EditorNavigationEntry[];
};

export type EditorGroupState = {
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
};

export function openNodeInActiveGroupState(state: EditorGroupState, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group, index) => (index === state.activeGroupIndex ? openNodeInEditorGroup(group, node) : group))
  };
}

export function openNodeInGroupState(state: EditorGroupState, groupId: number, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group) => (group.id === groupId ? openNodeInEditorGroup(group, node) : group))
  };
}

export function addEditorGroupState(state: EditorGroupState): Partial<EditorGroupState> {
  const active = state.editorGroups[state.activeGroupIndex];
  return appendEditorGroupState(state, active?.node ?? null);
}

export function openNodeInNewGroupState(state: EditorGroupState, node: RestNode): Partial<EditorGroupState> {
  return appendEditorGroupState(state, node);
}

function appendEditorGroupState(state: EditorGroupState, node: RestNode | null): Partial<EditorGroupState> {
  if (state.editorGroups.length >= MAX_EDITOR_GROUPS) return {};
  const editorGroups = [...state.editorGroups, createEditorGroup(state.nextGroupId, node)];
  return { editorGroups, activeGroupIndex: editorGroups.length - 1, nextGroupId: state.nextGroupId + 1 };
}

export function closeEditorGroupState(state: EditorGroupState, index: number): Partial<EditorGroupState> {
  if (state.editorGroups.length <= 1) return {};
  const editorGroups = state.editorGroups.filter((_, i) => i !== index);
  const activeGroupIndex = Math.max(0, Math.min(state.activeGroupIndex - (index <= state.activeGroupIndex ? 1 : 0), editorGroups.length - 1));
  return { editorGroups, activeGroupIndex };
}

export function updateEditorGroupNodeState(editorGroups: EditorGroup[], node: RestNode): EditorGroup[] {
  const updatedEntry = navigationEntry(node);
  return editorGroups.map((group) => ({
    ...group,
    node: group.node?.id === node.id ? node : group.node,
    back: group.back.map((entry) => entry.nodeId === node.id ? updatedEntry : entry),
    forward: group.forward.map((entry) => entry.nodeId === node.id ? updatedEntry : entry)
  }));
}

export function clearEditorGroupNodeState(editorGroups: EditorGroup[], nodeId: string): EditorGroup[] {
  return editorGroups.map((group) => ({
    ...group,
    node: group.node?.id === nodeId ? null : group.node,
    mode: group.node?.id === nodeId ? "preview" : group.mode,
    back: group.back.filter((entry) => entry.nodeId !== nodeId),
    forward: group.forward.filter((entry) => entry.nodeId !== nodeId)
  }));
}

export function setEditorGroupModeState(editorGroups: EditorGroup[], index: number, mode: "preview" | "edit"): EditorGroup[] {
  return editorGroups.map((group, i) => (i === index ? { ...group, mode } : group));
}

export function resetEditorGroupsState(state: Pick<EditorGroupState, "nextGroupId">): EditorGroupState {
  return {
    editorGroups: [createEditorGroup(state.nextGroupId, null)],
    activeGroupIndex: 0,
    nextGroupId: state.nextGroupId + 1
  };
}

export function navigationTarget(group: EditorGroup, direction: EditorNavigationDirection): EditorNavigationEntry | null {
  const entries = group[direction];
  return entries[entries.length - 1] ?? null;
}

export function navigateEditorGroupState(
  editorGroups: EditorGroup[],
  groupId: number,
  direction: EditorNavigationDirection,
  expectedNodeId: string,
  node: RestNode
): EditorGroup[] {
  return editorGroups.map((group) => {
    if (group.id !== groupId || navigationTarget(group, direction)?.nodeId !== expectedNodeId) return group;

    const source = group[direction].slice(0, -1);
    const opposite = direction === "back" ? "forward" : "back";
    const nextOpposite = group.node ? pushNavigationEntry(group[opposite], group.node) : group[opposite];
    return {
      ...group,
      node,
      mode: "preview",
      [direction]: source,
      [opposite]: nextOpposite
    };
  });
}

export function discardEditorNavigationTargetState(
  editorGroups: EditorGroup[],
  groupId: number,
  direction: EditorNavigationDirection,
  expectedNodeId: string
): EditorGroup[] {
  return editorGroups.map((group) => {
    if (group.id !== groupId || navigationTarget(group, direction)?.nodeId !== expectedNodeId) return group;
    return { ...group, [direction]: group[direction].slice(0, -1) };
  });
}

function openNodeInEditorGroup(group: EditorGroup, node: RestNode): EditorGroup {
  if (group.node?.id === node.id) return { ...group, node, mode: "preview" };
  return {
    ...group,
    node,
    mode: "preview",
    back: group.node ? pushNavigationEntry(group.back, group.node) : group.back,
    forward: []
  };
}

function createEditorGroup(id: number, node: RestNode | null): EditorGroup {
  return { id, node, mode: "preview", back: [], forward: [] };
}

function pushNavigationEntry(entries: EditorNavigationEntry[], node: RestNode): EditorNavigationEntry[] {
  return [...entries, navigationEntry(node)].slice(-(MAX_EDITOR_NAVIGATION_ENTRIES - 1));
}

function navigationEntry(node: RestNode): EditorNavigationEntry {
  return {
    spaceId: node.space_id,
    nodeId: node.id,
    nameSnapshot: node.name,
    kind: node.kind
  };
}
