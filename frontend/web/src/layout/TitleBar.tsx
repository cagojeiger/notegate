import { Columns2, Columns3, Moon, PanelLeft, PanelRight, Square, Sun } from "lucide-react";

import { MAX_EDITOR_GROUPS } from "../stores/uiStore";
import type { ThemeMode } from "../design/tokens";
import { IconButton } from "../shared/ui";
import type { Space } from "../api/types";

export function TitleBar({ activeSpace, theme, primarySidebarOpen, auxiliaryOpen, editorGroupCount, onAddGroup, onToggleTheme, onTogglePrimarySidebar, onToggleAuxiliary }: { activeSpace: Space | null; theme: ThemeMode; primarySidebarOpen: boolean; auxiliaryOpen: boolean; editorGroupCount: number; onAddGroup: () => void; onToggleTheme: () => void; onTogglePrimarySidebar: () => void; onToggleAuxiliary: () => void }) {
  const atMaxGroups = editorGroupCount >= MAX_EDITOR_GROUPS;
  // Split icon mirrors the current pane count so 1→2→3 reads at a glance.
  const SplitIcon = editorGroupCount >= 3 ? Columns3 : editorGroupCount === 2 ? Columns2 : Square;
  const splitLabel = atMaxGroups ? `Maximum ${MAX_EDITOR_GROUPS} editor groups` : `Split editor (${editorGroupCount}/${MAX_EDITOR_GROUPS})`;
  return (
    <header className="flex h-12 shrink-0 items-center justify-between border-b border-seam bg-surface px-3 max-md:h-[calc(3rem+env(safe-area-inset-top))] max-md:pt-[env(safe-area-inset-top)]">
      <div className="flex min-w-0 items-center gap-2">
        <div className="grid size-7 place-items-center rounded-xl bg-text text-sm font-semibold text-bg">N</div>
        <span className="font-semibold tracking-tight">Notegate</span>
        {activeSpace ? <span className="truncate text-sm text-muted">/ {activeSpace.name}</span> : null}
      </div>
      <div className="flex items-center gap-2 text-muted">
        <div className="flex items-center gap-1">
          <IconButton label="Toggle left sidebar" onClick={onTogglePrimarySidebar} pressed={primarySidebarOpen}><PanelLeft size={16} /></IconButton>
          <div className="hidden md:block">
            <IconButton label={splitLabel} onClick={onAddGroup} disabled={atMaxGroups} pressed={editorGroupCount > 1}><SplitIcon size={16} /></IconButton>
          </div>
          <IconButton label="Toggle right sidebar" onClick={onToggleAuxiliary} pressed={auxiliaryOpen}><PanelRight size={16} /></IconButton>
        </div>
        <div className="h-5 w-px bg-seam" aria-hidden="true" />
        <IconButton label="Toggle theme" onClick={onToggleTheme}>{theme === "light" ? <Moon size={16} /> : <Sun size={16} />}</IconButton>
      </div>
    </header>
  );
}
