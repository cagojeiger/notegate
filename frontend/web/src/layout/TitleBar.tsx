import { LayoutPanelLeft, Moon, PanelRight, Sun } from "lucide-react";

import type { ThemeMode } from "../design/tokens";
import { IconButton } from "../shared/ui";
import type { Space } from "../api/types";

export function TitleBar({ activeSpace, theme, primarySidebarOpen, auxiliaryOpen, onToggleTheme, onTogglePrimarySidebar, onToggleAuxiliary }: { activeSpace: Space | null; theme: ThemeMode; primarySidebarOpen: boolean; auxiliaryOpen: boolean; onToggleTheme: () => void; onTogglePrimarySidebar: () => void; onToggleAuxiliary: () => void }) {
  return (
    <header className="flex h-12 items-center justify-between border-b border-border bg-surface px-3">
      <div className="flex min-w-0 items-center gap-2">
        <div className="grid size-7 place-items-center rounded-xl bg-primary text-sm font-semibold text-primary-contrast shadow-[var(--ng-inset-shadow)]">N</div>
        <span className="font-semibold tracking-tight">Notegate</span>
        {activeSpace ? <span className="truncate text-sm text-muted">/ {activeSpace.name}</span> : null}
      </div>
      <div className="flex items-center gap-1 text-muted">
        <IconButton label="Toggle primary sidebar" onClick={onTogglePrimarySidebar} pressed={primarySidebarOpen}><LayoutPanelLeft size={16} /></IconButton>
        <IconButton label="Toggle theme" onClick={onToggleTheme}>{theme === "light" ? <Moon size={16} /> : <Sun size={16} />}</IconButton>
        <IconButton label="Toggle auxiliary sidebar" onClick={onToggleAuxiliary} pressed={auxiliaryOpen}><PanelRight size={16} /></IconButton>
      </div>
    </header>
  );
}
