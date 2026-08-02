import type { ThemeMode } from "../design/tokens";
import type { EditorGroup, EditorGroupState } from "./uiStoreReducers";
import {
  persistSpaceWorkbench,
  persistWorkbenchPanelState,
  restoreSpaceWorkbench,
  restoreWorkbenchPanelState,
  type WorkbenchPanelState
} from "./workbenchStorage";

const THEME_KEY = "notegate.theme";
const LAST_SPACE_KEY = "notegate.lastActiveSpaceId";

function readLocalStorage(key: string): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeLocalStorage(key: string, value: string): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Browser-local UI state is best-effort.
  }
}

export type UiStorePersistence = {
  loadTheme: () => ThemeMode;
  applyTheme: (theme: ThemeMode) => void;
  saveTheme: (theme: ThemeMode) => void;
  loadLastActiveSpaceId: () => string | null;
  saveLastActiveSpaceId: (spaceId: string) => void;
  loadSpaceWorkbench: (spaceId: string, nextGroupId: number) => EditorGroupState;
  saveSpaceWorkbench: (spaceId: string, editorGroups: EditorGroup[], activeGroupIndex: number) => void;
  loadPanelState: () => WorkbenchPanelState;
  savePanelState: (state: WorkbenchPanelState) => void;
};

export const browserUiStorePersistence: UiStorePersistence = {
  loadTheme: () => {
    if (typeof window === "undefined") return "dark";
    const stored = readLocalStorage(THEME_KEY);
    if (stored === "light" || stored === "dark") return stored;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  },
  applyTheme: (theme) => {
    if (typeof document !== "undefined") document.documentElement.dataset.theme = theme;
  },
  saveTheme: (theme) => {
    document.documentElement.dataset.theme = theme;
    writeLocalStorage(THEME_KEY, theme);
  },
  loadLastActiveSpaceId: () => readLocalStorage(LAST_SPACE_KEY),
  saveLastActiveSpaceId: (spaceId) => {
    writeLocalStorage(LAST_SPACE_KEY, spaceId);
  },
  loadSpaceWorkbench: restoreSpaceWorkbench,
  saveSpaceWorkbench: persistSpaceWorkbench,
  loadPanelState: restoreWorkbenchPanelState,
  savePanelState: persistWorkbenchPanelState
};
