import { Columns2, LayoutPanelLeft, Moon, PanelRight, Sun } from "lucide-react";

import { MAX_EDITOR_GROUPS } from "../stores/uiStore";
import type { ThemeMode } from "../design/tokens";
import { IconButton } from "../shared/ui";
import type { Space } from "../api/types";

export function TitleBar({ activeSpace, theme, primarySidebarOpen, auxiliaryOpen, editorGroupCount, onAddGroup, onToggleTheme, onTogglePrimarySidebar, onToggleAuxiliary }: { activeSpace: Space | null; theme: ThemeMode; primarySidebarOpen: boolean; auxiliaryOpen: boolean; editorGroupCount: number; onAddGroup: () => void; onToggleTheme: () => void; onTogglePrimarySidebar: () => void; onToggleAuxiliary: () => void }) {
  const atMaxGroups = editorGroupCount >= MAX_EDITOR_GROUPS;
  return (
    <header className="flex h-12 items-center justify-between border-b border-seam bg-surface px-3">
      <div className="flex min-w-0 items-center gap-2">
        <div className="grid size-7 place-items-center rounded-xl bg-primary text-sm font-semibold text-primary-contrast shadow-[var(--ng-inset-shadow)]">N</div>
        <span className="font-semibold tracking-tight">Notegate</span>
        {activeSpace ? <span className="truncate text-sm text-muted">/ {activeSpace.name}</span> : null}
      </div>
      <div className="flex items-center gap-2 text-muted">
        <div className="flex items-center gap-1">
          <IconButton label="Toggle primary sidebar" onClick={onTogglePrimarySidebar} pressed={primarySidebarOpen}><LayoutPanelLeft size={16} /></IconButton>
          <div className="hidden md:block">
            <IconButton label={atMaxGroups ? "Maximum editor groups" : "Add editor group"} onClick={onAddGroup} disabled={atMaxGroups}><Columns2 size={16} /></IconButton>
          </div>
          <IconButton label="Toggle auxiliary sidebar" onClick={onToggleAuxiliary} pressed={auxiliaryOpen}><PanelRight size={16} /></IconButton>
        </div>
        <div className="h-5 w-px bg-seam" aria-hidden="true" />
        <IconButton label="Toggle theme" onClick={onToggleTheme}>{theme === "light" ? <Moon size={16} /> : <Sun size={16} />}</IconButton>
      </div>
    </header>
  );
}
