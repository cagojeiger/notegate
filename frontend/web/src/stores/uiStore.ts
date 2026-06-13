import { create } from "zustand";

import type { RestNode } from "../api/types";
import type { ThemeMode } from "../design/tokens";

const THEME_KEY = "notegate.theme";
const LAST_SPACE_KEY = "notegate.lastActiveSpaceId";
export const MAX_EDITOR_GROUPS = 3;

function initialTheme(): ThemeMode {
  if (typeof window === "undefined") return "dark";
  const stored = window.localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function initialActiveSpaceId(): string | null {
  if (typeof window === "undefined") return null;
  return window.localStorage.getItem(LAST_SPACE_KEY);
}

// An EditorGroup is an independent pane. It owns the node it shows and its own
// preview/edit mode, so each group toggles independently of the others.
export type EditorGroup = { id: number; node: RestNode | null; mode: "preview" | "edit" };

type EditorGroupState = {
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
};

function openNodeInActiveGroupState(state: EditorGroupState, node: RestNode): Pick<EditorGroupState, "editorGroups"> {
  return {
    editorGroups: state.editorGroups.map((group, index) => (index === state.activeGroupIndex ? { ...group, node, mode: "preview" } : group))
  };
}

function addEditorGroupState(state: EditorGroupState): Partial<EditorGroupState> {
  if (state.editorGroups.length >= MAX_EDITOR_GROUPS) return {};
  const active = state.editorGroups[state.activeGroupIndex];
  const editorGroups = [...state.editorGroups, { id: state.nextGroupId, node: active?.node ?? null, mode: "preview" as const }];
  return { editorGroups, activeGroupIndex: editorGroups.length - 1, nextGroupId: state.nextGroupId + 1 };
}

function closeEditorGroupState(state: EditorGroupState, index: number): Partial<EditorGroupState> {
  if (state.editorGroups.length <= 1) return {};
  const editorGroups = state.editorGroups.filter((_, i) => i !== index);
  const activeGroupIndex = Math.max(0, Math.min(state.activeGroupIndex - (index <= state.activeGroupIndex ? 1 : 0), editorGroups.length - 1));
  return { editorGroups, activeGroupIndex };
}

function updateEditorGroupNodeState(editorGroups: EditorGroup[], node: RestNode): EditorGroup[] {
  return editorGroups.map((group) => (group.node?.id === node.id ? { ...group, node } : group));
}

function clearEditorGroupNodeState(editorGroups: EditorGroup[], nodeId: string): EditorGroup[] {
  return editorGroups.map((group) => (group.node?.id === nodeId ? { ...group, node: null, mode: "preview" } : group));
}

function setEditorGroupModeState(editorGroups: EditorGroup[], index: number, mode: "preview" | "edit"): EditorGroup[] {
  return editorGroups.map((group, i) => (i === index ? { ...group, mode } : group));
}

function resetEditorGroupsState(state: Pick<EditorGroupState, "nextGroupId">): EditorGroupState {
  return {
    editorGroups: [{ id: state.nextGroupId, node: null, mode: "preview" }],
    activeGroupIndex: 0,
    nextGroupId: state.nextGroupId + 1
  };
}

type UiState = {
  theme: ThemeMode;
  activeSpaceId: string | null;
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
  expandedFolderIds: Set<string>;
  primarySidebarOpen: boolean;
  primaryWidth: number;
  treeRatio: number;
  treeSectionOpen: boolean;
  recentSectionOpen: boolean;
  recentDensity: "list" | "compact";
  auxiliaryOpen: boolean;
  mobileTreeOpen: boolean;
  mobileAuxOpen: boolean;
  toast: string | null;
  saveState: "idle" | "saving" | "saved" | "error" | "conflict";
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  setActiveSpaceId: (id: string | null) => void;
  openInActiveGroup: (node: RestNode) => void;
  addGroup: () => void;
  closeGroup: (index: number) => void;
  focusGroup: (index: number) => void;
  updateGroupsNode: (node: RestNode) => void;
  clearGroupsWithNode: (nodeId: string) => void;
  setGroupMode: (index: number, mode: "preview" | "edit") => void;
  resetGroups: () => void;
  toggleFolder: (id: string) => void;
  addExpanded: (ids: string[]) => void;
  setExpanded: (ids: string[]) => void;
  togglePrimarySidebar: () => void;
  setPrimaryWidth: (width: number) => void;
  setTreeRatio: (ratio: number) => void;
  toggleTreeSection: () => void;
  toggleRecentSection: () => void;
  toggleRecentDensity: () => void;
  toggleAuxiliary: () => void;
  toggleMobileTree: () => void;
  toggleMobileAux: () => void;
  closeMobile: () => void;
  showToast: (message: string) => void;
  clearToast: () => void;
  setSaveState: (saveState: "idle" | "saving" | "saved" | "error" | "conflict") => void;
};

export const useUiStore = create<UiState>((set) => ({
  theme: initialTheme(),
  activeSpaceId: initialActiveSpaceId(),
  editorGroups: [{ id: 0, node: null, mode: "preview" }],
  activeGroupIndex: 0,
  nextGroupId: 1,
  expandedFolderIds: new Set(),
  primarySidebarOpen: true,
  primaryWidth: 300,
  treeRatio: 0.67,
  treeSectionOpen: true,
  recentSectionOpen: true,
  recentDensity: "list",
  auxiliaryOpen: true,
  mobileTreeOpen: false,
  mobileAuxOpen: false,
  toast: null,
  saveState: "idle",
  setTheme: (theme) => set({ theme }),
  toggleTheme: () => set((state) => ({ theme: state.theme === "light" ? "dark" : "light" })),
  setActiveSpaceId: (activeSpaceId) => set({ activeSpaceId }),
  openInActiveGroup: (node) => set((state) => openNodeInActiveGroupState(state, node)),
  addGroup: () => set((state) => addEditorGroupState(state)),
  closeGroup: (index) => set((state) => closeEditorGroupState(state, index)),
  focusGroup: (index) => set({ activeGroupIndex: index }),
  updateGroupsNode: (node) => set((state) => ({ editorGroups: updateEditorGroupNodeState(state.editorGroups, node) })),
  clearGroupsWithNode: (nodeId) => set((state) => ({ editorGroups: clearEditorGroupNodeState(state.editorGroups, nodeId) })),
  setGroupMode: (index, mode) => set((state) => ({ editorGroups: setEditorGroupModeState(state.editorGroups, index, mode) })),
  resetGroups: () => set((state) => resetEditorGroupsState(state)),
  toggleFolder: (id) =>
    set((state) => {
      const next = new Set(state.expandedFolderIds);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return { expandedFolderIds: next };
    }),
  addExpanded: (ids) =>
    set((state) => {
      const next = new Set(state.expandedFolderIds);
      for (const id of ids) next.add(id);
      return { expandedFolderIds: next };
    }),
  setExpanded: (ids) => set({ expandedFolderIds: new Set(ids) }),
  togglePrimarySidebar: () => set((state) => ({ primarySidebarOpen: !state.primarySidebarOpen })),
  setPrimaryWidth: (width) => set({ primaryWidth: Math.max(220, Math.min(520, Math.round(width))) }),
  setTreeRatio: (ratio) => set({ treeRatio: Math.max(0.2, Math.min(0.82, ratio)) }),
  toggleTreeSection: () => set((state) => ({ treeSectionOpen: !state.treeSectionOpen })),
  toggleRecentSection: () => set((state) => ({ recentSectionOpen: !state.recentSectionOpen })),
  toggleRecentDensity: () => set((state) => ({ recentDensity: state.recentDensity === "list" ? "compact" : "list" })),
  toggleAuxiliary: () => set((state) => ({ auxiliaryOpen: !state.auxiliaryOpen })),
  toggleMobileTree: () => set((state) => ({ mobileTreeOpen: !state.mobileTreeOpen })),
  toggleMobileAux: () => set((state) => ({ mobileAuxOpen: !state.mobileAuxOpen })),
  closeMobile: () => set({ mobileTreeOpen: false, mobileAuxOpen: false }),
  showToast: (toast) => set({ toast }),
  clearToast: () => set({ toast: null }),
  setSaveState: (saveState) => set({ saveState })
}));

export function persistTheme(theme: ThemeMode): void {
  document.documentElement.dataset.theme = theme;
  window.localStorage.setItem(THEME_KEY, theme);
}

export function persistLastSpace(spaceId: string): void {
  window.localStorage.setItem(LAST_SPACE_KEY, spaceId);
}
