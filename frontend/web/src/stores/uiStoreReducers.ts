import type { RestNode } from "../api/types";

export const MAX_EDITOR_GROUPS = 3;

export type OpenedNodeRef = {
  nodeId: string;
  spaceId: string;
};

// Server node data stays in React Query. A group only owns the identity of the
// node it shows and its preview/edit mode.
export type EditorGroup = { id: number; nodeRef: OpenedNodeRef | null; mode: "preview" | "edit" };

export type EditorGroupState = {
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
};

export function openNodeInActiveGroupState(state: EditorGroupState, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group, index) => (
      index === state.activeGroupIndex ? { ...group, nodeRef: toOpenedNodeRef(node), mode: "preview" } : group
    ))
  };
}

export function openNodeInGroupState(state: EditorGroupState, groupId: number, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group) => (
      group.id === groupId ? { ...group, nodeRef: toOpenedNodeRef(node), mode: "preview" } : group
    ))
  };
}

export function addEditorGroupState(state: EditorGroupState): Partial<EditorGroupState> {
  const active = state.editorGroups[state.activeGroupIndex];
  return appendEditorGroupState(state, active?.nodeRef ?? null);
}

export function openNodeInNewGroupState(state: EditorGroupState, node: RestNode): Partial<EditorGroupState> {
  return appendEditorGroupState(state, toOpenedNodeRef(node));
}

function appendEditorGroupState(state: EditorGroupState, nodeRef: OpenedNodeRef | null): Partial<EditorGroupState> {
  if (state.editorGroups.length >= MAX_EDITOR_GROUPS) return {};
  const editorGroups = [...state.editorGroups, { id: state.nextGroupId, nodeRef, mode: "preview" as const }];
  return { editorGroups, activeGroupIndex: editorGroups.length - 1, nextGroupId: state.nextGroupId + 1 };
}

export function closeEditorGroupState(state: EditorGroupState, index: number): Partial<EditorGroupState> {
  if (state.editorGroups.length <= 1) return {};
  const editorGroups = state.editorGroups.filter((_, i) => i !== index);
  const activeGroupIndex = Math.max(0, Math.min(state.activeGroupIndex - (index <= state.activeGroupIndex ? 1 : 0), editorGroups.length - 1));
  return { editorGroups, activeGroupIndex };
}

export function clearEditorGroupNodeState(editorGroups: EditorGroup[], nodeId: string): EditorGroup[] {
  return editorGroups.map((group) => (
    group.nodeRef?.nodeId === nodeId ? { ...group, nodeRef: null, mode: "preview" } : group
  ));
}

export function setEditorGroupModeState(editorGroups: EditorGroup[], index: number, mode: "preview" | "edit"): EditorGroup[] {
  return editorGroups.map((group, i) => (i === index ? { ...group, mode } : group));
}

export function resetEditorGroupsState(state: Pick<EditorGroupState, "nextGroupId">): EditorGroupState {
  return {
    editorGroups: [{ id: state.nextGroupId, nodeRef: null, mode: "preview" }],
    activeGroupIndex: 0,
    nextGroupId: state.nextGroupId + 1
  };
}

function toOpenedNodeRef(node: RestNode): OpenedNodeRef {
  return { nodeId: node.id, spaceId: node.space_id };
}
