import { Columns2, Columns3, Moon, PanelLeft, PanelRight, Square, Sun } from "lucide-react";

import type { Space } from "../api/types";
import type { ThemeMode } from "../design/tokens";
import { MAX_EDITOR_GROUPS } from "../shared/model/workbenchLayout";
import { BrandAppIcon, IconButton } from "../shared/ui";

type TitleBarProps = {
  activeSpace: Space | null;
  locationLabel?: string;
  showWorkbenchControls?: boolean;
  theme: ThemeMode;
  primarySidebarOpen: boolean;
  auxiliaryOpen: boolean;
  auxiliaryLabel?: string;
  editorGroupCount: number;
  onAddGroup: () => void;
  onToggleTheme: () => void;
  onTogglePrimarySidebar: () => void;
  onToggleAuxiliary: () => void;
};

export function TitleBar({
  activeSpace,
  locationLabel,
  showWorkbenchControls = true,
  theme,
  primarySidebarOpen,
  auxiliaryOpen,
  auxiliaryLabel = "Toggle right sidebar",
  editorGroupCount,
  onAddGroup,
  onToggleTheme,
  onTogglePrimarySidebar,
  onToggleAuxiliary
}: TitleBarProps) {
  const atMaxGroups = editorGroupCount >= MAX_EDITOR_GROUPS;
  // Split icon mirrors the current pane count so 1→2→3 reads at a glance.
  const SplitIcon = editorGroupCount >= 3 ? Columns3 : editorGroupCount === 2 ? Columns2 : Square;
  const splitLabel = atMaxGroups ? `Maximum ${MAX_EDITOR_GROUPS} editor groups` : `Split editor (${editorGroupCount}/${MAX_EDITOR_GROUPS})`;

  return (
    <header className="grid h-12 shrink-0 grid-cols-[52px_minmax(0,1fr)_auto] items-center border-b border-seam bg-surface max-md:h-[calc(3rem+env(safe-area-inset-top))] max-md:grid-cols-[minmax(0,1fr)_auto] max-md:px-3 max-md:pt-[env(safe-area-inset-top)]">
      <div className="grid h-full place-items-center max-md:hidden">
        <BrandAppIcon size={28} decorative />
      </div>
      <div className="flex min-w-0 items-center gap-2 px-3 max-md:px-0">
        <BrandAppIcon size={28} className="md:hidden" decorative />
        <span className="font-semibold tracking-tight">NoteGate</span>
        {locationLabel || activeSpace ? <span className="truncate text-sm text-muted">/ {locationLabel ?? activeSpace?.name}</span> : null}
      </div>
      <div className="flex items-center gap-2 pr-3 text-muted max-md:pr-0">
        <div className="flex items-center gap-1">
          {showWorkbenchControls ? (
            <>
              <IconButton label="Toggle left sidebar" onClick={onTogglePrimarySidebar} pressed={primarySidebarOpen}><PanelLeft size={16} /></IconButton>
              <div className="hidden md:block">
                <IconButton label={splitLabel} onClick={onAddGroup} disabled={atMaxGroups} pressed={editorGroupCount > 1}><SplitIcon size={16} /></IconButton>
              </div>
            </>
          ) : null}
          <IconButton label={auxiliaryLabel} onClick={onToggleAuxiliary} pressed={auxiliaryOpen}><PanelRight size={16} /></IconButton>
        </div>
        <IconButton label="Toggle theme" onClick={onToggleTheme}>{theme === "light" ? <Moon size={16} /> : <Sun size={16} />}</IconButton>
      </div>
    </header>
  );
}
