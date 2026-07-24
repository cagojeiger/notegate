import { create } from "zustand";

import type { RestNode } from "../api/types";
import type { ThemeMode } from "../design/tokens";
import { WORKBENCH_LAYOUT } from "../layout/workbenchLayout";
import { addEditorGroupState, clearEditorGroupNodeState, closeEditorGroupState, MAX_EDITOR_GROUPS, openNodeInActiveGroupState, openNodeInGroupState, openNodeInNewGroupState, resetEditorGroupsState, setEditorGroupModeState, updateEditorGroupNodeState, type EditorGroup } from "./uiStoreReducers";
import { persistSpaceWorkbench, persistWorkbenchPanelState, readLastActiveSpace, restoreSpaceWorkbench, restoreWorkbenchPanelState } from "./workbenchStorage";

export { MAX_EDITOR_GROUPS };
export type { EditorGroup };

const THEME_KEY = "notegate.theme";

function initialTheme(): ThemeMode {
  if (typeof window === "undefined") return "dark";
  const stored = window.localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

const initialThemeMode = initialTheme();
if (typeof document !== "undefined") document.documentElement.dataset.theme = initialThemeMode;

const initialSpaceId = readLastActiveSpace();
const initialEditorState = initialSpaceId ? restoreSpaceWorkbench(initialSpaceId, 0) : resetEditorGroupsState({ nextGroupId: 0 });
const initialPanelState = restoreWorkbenchPanelState();
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
  openInGroup: (groupId: number, node: RestNode) => void;
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
  resetWorkbenchSession: () => void;
};

export const useUiStore = create<UiState>((set, get) => ({
  theme: initialThemeMode,
  activeSpaceId: initialSpaceId,
  editorGroups: initialEditorState.editorGroups,
  activeGroupIndex: initialEditorState.activeGroupIndex,
  nextGroupId: initialEditorState.nextGroupId,
  expandedFolderIds: new Set(),
  primarySidebarOpen: initialPanelState.primarySidebarOpen,
  primaryWidth: WORKBENCH_LAYOUT.defaultPrimaryWidth,
  treeRatio: WORKBENCH_LAYOUT.defaultTreeRatio,
  treeSectionOpen: true,
  recentSectionOpen: true,
  recentDensity: "list",
  auxiliaryOpen: initialPanelState.auxiliaryOpen,
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
  openInGroup: (groupId, node) => set((state) => openNodeInGroupState(state, groupId, node)),
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
  togglePrimarySidebar: () =>
    set((state) => {
      const primarySidebarOpen = !state.primarySidebarOpen;
      persistWorkbenchPanelState({ primarySidebarOpen, auxiliaryOpen: state.auxiliaryOpen });
      return { primarySidebarOpen };
    }),
  setPrimaryWidth: (width) => set({ primaryWidth: Math.max(WORKBENCH_LAYOUT.minPrimaryWidth, Math.min(WORKBENCH_LAYOUT.maxPrimaryWidth, Math.round(width))) }),
  setTreeRatio: (ratio) => set({ treeRatio: Math.max(WORKBENCH_LAYOUT.minTreeRatio, Math.min(WORKBENCH_LAYOUT.maxTreeRatio, ratio)) }),
  toggleTreeSection: () => set((state) => ({ treeSectionOpen: !state.treeSectionOpen })),
  toggleRecentSection: () => set((state) => ({ recentSectionOpen: !state.recentSectionOpen })),
  toggleRecentDensity: () => set((state) => ({ recentDensity: state.recentDensity === "list" ? "compact" : "list" })),
  toggleAuxiliary: () =>
    set((state) => {
      const auxiliaryOpen = !state.auxiliaryOpen;
      persistWorkbenchPanelState({ primarySidebarOpen: state.primarySidebarOpen, auxiliaryOpen });
      return { auxiliaryOpen };
    }),
  toggleMobileTree: () => set((state) => ({ mobileTreeOpen: !state.mobileTreeOpen })),
  toggleMobileAux: () => set((state) => ({ mobileAuxOpen: !state.mobileAuxOpen })),
  closeMobile: () => set({ mobileTreeOpen: false, mobileAuxOpen: false }),
  showToast: (toast) => set({ toast }),
  clearToast: () => set({ toast: null }),
  setSaveState: (saveState) => set({ saveState }),
  resetWorkbenchSession: () => set((state) => ({
    activeSpaceId: null,
    ...resetEditorGroupsState({ nextGroupId: state.nextGroupId }),
    expandedFolderIds: new Set(),
    primarySidebarOpen: true,
    primaryWidth: WORKBENCH_LAYOUT.defaultPrimaryWidth,
    treeRatio: WORKBENCH_LAYOUT.defaultTreeRatio,
    treeSectionOpen: true,
    recentSectionOpen: true,
    recentDensity: "list",
    auxiliaryOpen: true,
    mobileTreeOpen: false,
    mobileAuxOpen: false,
    toast: null,
    saveState: "idle"
  }))
}));

export function persistTheme(theme: ThemeMode): void {
  document.documentElement.dataset.theme = theme;
  window.localStorage.setItem(THEME_KEY, theme);
}
