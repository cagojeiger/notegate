import { History, LayoutGrid, Plus, Settings } from "lucide-react";

import type { Space } from "../../api/types";

// Mobile presentation of the ActivityRail: a bottom space switcher bar.
// Space list scrolls; ＋ hugs the list end; History and Settings stay at the far right.
export function MobileSpaceBar({ spaces, activeSpace, canCreateSpace, navigationLocked = false, onSelectSpace, onCreateSpace, onOpenLibrary, libraryActive = false, onOpenHistory, onOpenSettings }: { spaces: Space[]; activeSpace: Space | null; canCreateSpace: boolean; navigationLocked?: boolean; onSelectSpace: (space: Space) => void; onCreateSpace: () => void; onOpenLibrary?: () => void; libraryActive?: boolean; onOpenHistory: () => void; onOpenSettings: () => void }) {
  return (
    <nav aria-label="Spaces" className="flex h-[calc(3.5rem+env(safe-area-inset-bottom))] shrink-0 items-center gap-2 border-t border-seam bg-surface px-3 pb-[calc(0.5rem+env(safe-area-inset-bottom))] pt-2 md:hidden">
      {onOpenLibrary ? (
        <div className="relative shrink-0">
          {libraryActive ? <span data-active-indicator className="absolute -top-2 left-2 right-2 h-[3px] rounded-b-full bg-primary" aria-hidden="true" /> : null}
          <button
            type="button"
            disabled={navigationLocked}
            aria-label="Open space library"
            aria-pressed={libraryActive}
            aria-current={libraryActive ? "page" : undefined}
            onClick={onOpenLibrary}
            className={`grid size-9 place-items-center rounded-xl transition active:bg-[var(--ng-selection)] disabled:cursor-not-allowed disabled:opacity-45 ${libraryActive ? "bg-[var(--ng-selection)] text-primary" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}
          >
            <LayoutGrid size={16} />
          </button>
        </div>
      ) : null}
      <div className="flex min-w-0 flex-[0_1_auto] items-center gap-2 overflow-x-auto">
        {spaces.map((space) => {
          const active = !libraryActive && activeSpace?.id === space.id;
          return (
            <div key={space.id} className="relative shrink-0">
              {active ? <span data-active-indicator className="absolute -top-2 left-2 right-2 h-[3px] rounded-b-full bg-primary" aria-hidden="true" /> : null}
              <button
                type="button"
                disabled={navigationLocked}
                title={space.name}
                aria-label={space.name}
                aria-current={active ? "page" : undefined}
                onClick={() => onSelectSpace(space)}
                className={`grid size-9 place-items-center rounded-xl text-sm font-semibold transition active:bg-[var(--ng-selection)] disabled:cursor-not-allowed disabled:opacity-45 ${active ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}
              >
                {space.name.slice(0, 1).toUpperCase()}
              </button>
            </div>
          );
        })}
      </div>
      {canCreateSpace ? (
        <div className="shrink-0 border-l border-seam pl-2">
          <button type="button" aria-label="Add space" onClick={onCreateSpace} className="grid size-9 place-items-center rounded-xl text-muted hover:bg-[var(--ng-hover)] hover:text-text">
            <Plus size={16} />
          </button>
        </div>
      ) : null}
      <div className="ml-auto flex shrink-0 items-center gap-1 border-l border-seam pl-2">
        <button type="button" disabled={navigationLocked} aria-label="History" title="History" onClick={onOpenHistory} className="grid size-9 place-items-center rounded-xl text-muted hover:bg-[var(--ng-hover)] hover:text-text disabled:cursor-not-allowed disabled:opacity-45">
          <History size={16} />
        </button>
        <button type="button" disabled={navigationLocked} aria-label="Settings" onClick={onOpenSettings} className="grid size-9 place-items-center rounded-xl text-muted hover:bg-[var(--ng-hover)] hover:text-text disabled:cursor-not-allowed disabled:opacity-45">
          <Settings size={16} />
        </button>
      </div>
    </nav>
  );
}
