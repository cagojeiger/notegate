import { create } from "zustand";
import type { StoreApi, UseBoundStore } from "zustand";

import type { RestNode } from "../api/types";
import type { ThemeMode } from "../design/tokens";
import { WORKBENCH_LAYOUT } from "../shared/model/workbenchLayout";
import { addEditorGroupState, clearEditorGroupNodeState, closeEditorGroupState, discardEditorNavigationTargetState, navigateEditorGroupState, navigationTarget, openNodeInActiveGroupState, openNodeInGroupState, openNodeInNewGroupState, resetEditorGroupsState, setEditorGroupModeState, updateEditorGroupNodeState, type EditorGroup, type EditorNavigationDirection } from "./uiStoreReducers";
import { browserUiStorePersistence, type UiStorePersistence } from "./uiStorePersistence";

export type { EditorGroup };

type UiState = {
  theme: ThemeMode;
  activeSpaceId: string | null;
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
  expandedFolderIds: Set<string>;
  expandedFolderIdsBySpace: Record<string, string[]>;
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
  navigateGroup: (groupId: number, direction: EditorNavigationDirection, expectedNodeId: string, node: RestNode) => boolean;
  discardNavigationTarget: (groupId: number, direction: EditorNavigationDirection, expectedNodeId: string) => boolean;
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

type UiStore = UseBoundStore<StoreApi<UiState>>;

export function createUiStore(persistence: UiStorePersistence): UiStore {
  const initialEditorState = resetEditorGroupsState({ nextGroupId: 0 });
  return create<UiState>((set, get) => ({
  theme: "light",
  activeSpaceId: null,
  editorGroups: initialEditorState.editorGroups,
  activeGroupIndex: initialEditorState.activeGroupIndex,
  nextGroupId: initialEditorState.nextGroupId,
  expandedFolderIds: new Set(),
  expandedFolderIdsBySpace: {},
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
  saveState: "idle",
  toggleTheme: () => set((state) => ({ theme: state.theme === "light" ? "dark" : "light" })),
  setActiveSpaceId: (activeSpaceId) => {
    const state = get();
    if (state.activeSpaceId === activeSpaceId) return;
    if (state.activeSpaceId) persistence.saveSpaceWorkbench(state.activeSpaceId, state.editorGroups, state.activeGroupIndex);
    const expandedFolderIdsBySpace = rememberExpandedFolders(state);
    if (!activeSpaceId) {
      set({
        activeSpaceId,
        expandedFolderIds: new Set(),
        expandedFolderIdsBySpace,
        ...resetEditorGroupsState({ nextGroupId: state.nextGroupId })
      });
      return;
    }
    set({
      activeSpaceId,
      expandedFolderIds: new Set(expandedFolderIdsBySpace[activeSpaceId] ?? []),
      expandedFolderIdsBySpace,
      ...persistence.loadSpaceWorkbench(activeSpaceId, state.nextGroupId)
    });
  },
  openInActiveGroup: (node) => set((state) => openNodeInActiveGroupState(state, node)),
  openInGroup: (groupId, node) => set((state) => openNodeInGroupState(state, groupId, node)),
  openInNewGroup: (node) => set((state) => openNodeInNewGroupState(state, node)),
  addGroup: () => set((state) => addEditorGroupState(state)),
  closeGroup: (index) => set((state) => closeEditorGroupState(state, index)),
  focusGroup: (index) => set({ activeGroupIndex: index }),
  updateGroupsNode: (node) => set((state) => ({ editorGroups: updateEditorGroupNodeState(state.editorGroups, node) })),
  clearGroupsWithNode: (nodeId) => set((state) => ({ editorGroups: clearEditorGroupNodeState(state.editorGroups, nodeId) })),
  navigateGroup: (groupId, direction, expectedNodeId, node) => {
    const state = get();
    const group = state.editorGroups.find((candidate) => candidate.id === groupId);
    if (!group || navigationTarget(group, direction)?.nodeId !== expectedNodeId) return false;
    set({ editorGroups: navigateEditorGroupState(state.editorGroups, groupId, direction, expectedNodeId, node) });
    return true;
  },
  discardNavigationTarget: (groupId, direction, expectedNodeId) => {
    const state = get();
    const group = state.editorGroups.find((candidate) => candidate.id === groupId);
    if (!group || navigationTarget(group, direction)?.nodeId !== expectedNodeId) return false;
    set({ editorGroups: discardEditorNavigationTargetState(state.editorGroups, groupId, direction, expectedNodeId) });
    return true;
  },
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
      persistence.savePanelState({ primarySidebarOpen, auxiliaryOpen: state.auxiliaryOpen });
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
      persistence.savePanelState({ primarySidebarOpen: state.primarySidebarOpen, auxiliaryOpen });
      return { auxiliaryOpen };
    }),
  toggleMobileTree: () => set((state) => ({ mobileTreeOpen: !state.mobileTreeOpen })),
  toggleMobileAux: () => set((state) => ({ mobileAuxOpen: !state.mobileAuxOpen })),
  closeMobile: () => set({ mobileTreeOpen: false, mobileAuxOpen: false }),
  showToast: (toast) => set({ toast }),
  clearToast: () => set({ toast: null }),
  setSaveState: (saveState) => set({ saveState })
  }));
}

export function createHydratedUiStore(persistence: UiStorePersistence): UiStore {
  const store = createUiStore(persistence);
  hydrateUiStore(store, persistence);
  return store;
}

function hydrateUiStore(store: UiStore, persistence: UiStorePersistence): void {
  const theme = persistence.loadTheme();
  const activeSpaceId = persistence.loadLastActiveSpaceId();
  const editorState = activeSpaceId
    ? persistence.loadSpaceWorkbench(activeSpaceId, 0)
    : resetEditorGroupsState({ nextGroupId: 0 });
  const panelState = persistence.loadPanelState();

  persistence.applyTheme(theme);
  store.setState({
    theme,
    activeSpaceId,
    ...editorState,
    primarySidebarOpen: panelState.primarySidebarOpen,
    auxiliaryOpen: panelState.auxiliaryOpen
  });
}

export const useUiStore = createUiStore(browserUiStorePersistence);

export function bootstrapUiStore(): void {
  hydrateUiStore(useUiStore, browserUiStorePersistence);
}

function rememberExpandedFolders(
  state: Pick<UiState, "activeSpaceId" | "expandedFolderIds" | "expandedFolderIdsBySpace">
): Record<string, string[]> {
  if (!state.activeSpaceId) return state.expandedFolderIdsBySpace;
  return {
    ...state.expandedFolderIdsBySpace,
    [state.activeSpaceId]: [...state.expandedFolderIds]
  };
}

export function persistTheme(theme: ThemeMode): void {
  browserUiStorePersistence.saveTheme(theme);
}
