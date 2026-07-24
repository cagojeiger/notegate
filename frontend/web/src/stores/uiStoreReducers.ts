import type { RestNode } from "../api/types";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";

// An EditorGroup is an independent pane. It owns the node it shows and its own
// preview/edit mode, so each group toggles independently of the others.
export type EditorGroup = { id: number; node: RestNode | null; mode: "preview" | "edit" };

export type EditorGroupState = {
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
};

export function openNodeInActiveGroupState(state: EditorGroupState, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group, index) => (index === state.activeGroupIndex ? { ...group, node, mode: "preview" } : group))
  };
}

export function openNodeInGroupState(state: EditorGroupState, groupId: number, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group) => (group.id === groupId ? { ...group, node, mode: "preview" } : group))
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
  const editorGroups = [...state.editorGroups, { id: state.nextGroupId, node, mode: "preview" as const }];
  return { editorGroups, activeGroupIndex: editorGroups.length - 1, nextGroupId: state.nextGroupId + 1 };
}

export function closeEditorGroupState(state: EditorGroupState, index: number): Partial<EditorGroupState> {
  if (state.editorGroups.length <= 1) return {};
  const editorGroups = state.editorGroups.filter((_, i) => i !== index);
  const activeGroupIndex = Math.max(0, Math.min(state.activeGroupIndex - (index <= state.activeGroupIndex ? 1 : 0), editorGroups.length - 1));
  return { editorGroups, activeGroupIndex };
}

export function updateEditorGroupNodeState(editorGroups: EditorGroup[], node: RestNode): EditorGroup[] {
  return editorGroups.map((group) => (group.node?.id === node.id ? { ...group, node } : group));
}

export function clearEditorGroupNodeState(editorGroups: EditorGroup[], nodeId: string): EditorGroup[] {
  return editorGroups.map((group) => (group.node?.id === nodeId ? { ...group, node: null, mode: "preview" } : group));
}

export function setEditorGroupModeState(editorGroups: EditorGroup[], index: number, mode: "preview" | "edit"): EditorGroup[] {
  return editorGroups.map((group, i) => (i === index ? { ...group, mode } : group));
}

export function resetEditorGroupsState(state: Pick<EditorGroupState, "nextGroupId">): EditorGroupState {
  return {
    editorGroups: [{ id: state.nextGroupId, node: null, mode: "preview" }],
    activeGroupIndex: 0,
    nextGroupId: state.nextGroupId + 1
  };
}
