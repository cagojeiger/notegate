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

// An EditorGroup is an independent pane. It owns the node it shows; preview/edit
// mode is kept locally by the TextEditor instance so each group toggles on its own.
export type EditorGroup = { id: number; node: RestNode | null };

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
  auxiliaryOpen: boolean;
  mobileTreeOpen: boolean;
  mobileAuxOpen: boolean;
  toast: string | null;
  setTheme: (theme: ThemeMode) => void;
  toggleTheme: () => void;
  setActiveSpaceId: (id: string | null) => void;
  openInActiveGroup: (node: RestNode) => void;
  addGroup: () => void;
  closeGroup: (index: number) => void;
  focusGroup: (index: number) => void;
  updateGroupsNode: (node: RestNode) => void;
  clearGroupsWithNode: (nodeId: string) => void;
  resetGroups: () => void;
  toggleFolder: (id: string) => void;
  addExpanded: (ids: string[]) => void;
  setExpanded: (ids: string[]) => void;
  togglePrimarySidebar: () => void;
  setPrimaryWidth: (width: number) => void;
  setTreeRatio: (ratio: number) => void;
  toggleAuxiliary: () => void;
  toggleMobileTree: () => void;
  toggleMobileAux: () => void;
  closeMobile: () => void;
  showToast: (message: string) => void;
  clearToast: () => void;
};

export const useUiStore = create<UiState>((set) => ({
  theme: initialTheme(),
  activeSpaceId: initialActiveSpaceId(),
  editorGroups: [{ id: 0, node: null }],
  activeGroupIndex: 0,
  nextGroupId: 1,
  expandedFolderIds: new Set(),
  primarySidebarOpen: true,
  primaryWidth: 300,
  treeRatio: 0.62,
  auxiliaryOpen: true,
  mobileTreeOpen: false,
  mobileAuxOpen: false,
  toast: null,
  setTheme: (theme) => set({ theme }),
  toggleTheme: () => set((state) => ({ theme: state.theme === "light" ? "dark" : "light" })),
  setActiveSpaceId: (activeSpaceId) => set({ activeSpaceId }),
  openInActiveGroup: (node) =>
    set((state) => ({
      editorGroups: state.editorGroups.map((group, index) => (index === state.activeGroupIndex ? { ...group, node } : group))
    })),
  addGroup: () =>
    set((state) => {
      if (state.editorGroups.length >= MAX_EDITOR_GROUPS) return state;
      const active = state.editorGroups[state.activeGroupIndex];
      const groups = [...state.editorGroups, { id: state.nextGroupId, node: active?.node ?? null }];
      return { editorGroups: groups, activeGroupIndex: groups.length - 1, nextGroupId: state.nextGroupId + 1 };
    }),
  closeGroup: (index) =>
    set((state) => {
      if (state.editorGroups.length <= 1) return state;
      const groups = state.editorGroups.filter((_, i) => i !== index);
      const activeGroupIndex = Math.max(0, Math.min(state.activeGroupIndex - (index <= state.activeGroupIndex ? 1 : 0), groups.length - 1));
      return { editorGroups: groups, activeGroupIndex };
    }),
  focusGroup: (index) => set({ activeGroupIndex: index }),
  updateGroupsNode: (node) =>
    set((state) => ({
      editorGroups: state.editorGroups.map((group) => (group.node?.id === node.id ? { ...group, node } : group))
    })),
  clearGroupsWithNode: (nodeId) =>
    set((state) => ({
      editorGroups: state.editorGroups.map((group) => (group.node?.id === nodeId ? { ...group, node: null } : group))
    })),
  resetGroups: () => set((state) => ({ editorGroups: [{ id: state.nextGroupId, node: null }], activeGroupIndex: 0, nextGroupId: state.nextGroupId + 1 })),
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
  toggleAuxiliary: () => set((state) => ({ auxiliaryOpen: !state.auxiliaryOpen })),
  toggleMobileTree: () => set((state) => ({ mobileTreeOpen: !state.mobileTreeOpen })),
  toggleMobileAux: () => set((state) => ({ mobileAuxOpen: !state.mobileAuxOpen })),
  closeMobile: () => set({ mobileTreeOpen: false, mobileAuxOpen: false }),
  showToast: (toast) => set({ toast }),
  clearToast: () => set({ toast: null })
}));

export function persistTheme(theme: ThemeMode): void {
  document.documentElement.dataset.theme = theme;
  window.localStorage.setItem(THEME_KEY, theme);
}

export function persistLastSpace(spaceId: string): void {
  window.localStorage.setItem(LAST_SPACE_KEY, spaceId);
}
