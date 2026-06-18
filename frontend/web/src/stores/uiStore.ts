import { create } from "zustand";

import type { RestNode } from "../api/types";
import type { ThemeMode } from "../design/tokens";
import { addEditorGroupState, clearEditorGroupNodeState, closeEditorGroupState, MAX_EDITOR_GROUPS, openNodeInActiveGroupState, openNodeInNewGroupState, resetEditorGroupsState, setEditorGroupModeState, updateEditorGroupNodeState, type EditorGroup } from "./uiStoreReducers";
import { persistSpaceWorkbench, restoreSpaceWorkbench } from "./workbenchStorage";

export { MAX_EDITOR_GROUPS };
export type { EditorGroup };

const THEME_KEY = "notegate.theme";
const LAST_SPACE_KEY = "notegate.lastActiveSpaceId";

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

const initialSpaceId = initialActiveSpaceId();
const initialEditorState = initialSpaceId ? restoreSpaceWorkbench(initialSpaceId, 0) : resetEditorGroupsState({ nextGroupId: 0 });
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
  toggleTheme: () => void;
  setActiveSpaceId: (id: string | null) => void;
  openInActiveGroup: (node: RestNode) => void;
  openInNewGroup: (node: RestNode) => void;
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

export const useUiStore = create<UiState>((set, get) => ({
  theme: initialTheme(),
  activeSpaceId: initialSpaceId,
  editorGroups: initialEditorState.editorGroups,
  activeGroupIndex: initialEditorState.activeGroupIndex,
  nextGroupId: initialEditorState.nextGroupId,
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
  toggleTheme: () => set((state) => ({ theme: state.theme === "light" ? "dark" : "light" })),
  setActiveSpaceId: (activeSpaceId) => {
    const state = get();
    if (state.activeSpaceId === activeSpaceId) return;
    if (state.activeSpaceId) persistSpaceWorkbench(state.activeSpaceId, state.editorGroups, state.activeGroupIndex);
    if (!activeSpaceId) {
      set({ activeSpaceId, ...resetEditorGroupsState({ nextGroupId: state.nextGroupId }) });
      return;
    }
    set({ activeSpaceId, ...restoreSpaceWorkbench(activeSpaceId, state.nextGroupId) });
  },
  openInActiveGroup: (node) => set((state) => openNodeInActiveGroupState(state, node)),
  openInNewGroup: (node) => set((state) => openNodeInNewGroupState(state, node)),
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
