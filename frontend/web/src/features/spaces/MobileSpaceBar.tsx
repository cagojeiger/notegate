import { History, Plus, Settings } from "lucide-react";

import type { Space } from "../../api/types";

// Mobile presentation of the ActivityRail: a bottom space switcher bar.
// Space list scrolls; ＋ hugs the list end; Settings is pinned far-right (docs/ui 01-layout).
export function MobileSpaceBar({ spaces, activeSpace, canCreateSpace, transferCount, onSelectSpace, onCreateSpace, onOpenHistory, onOpenSettings }: { spaces: Space[]; activeSpace: Space | null; canCreateSpace: boolean; transferCount: number; onSelectSpace: (space: Space) => void; onCreateSpace: () => void; onOpenHistory: () => void; onOpenSettings: () => void }) {
  return (
    <nav aria-label="Spaces" className="flex h-[calc(3.5rem+env(safe-area-inset-bottom))] shrink-0 items-center gap-2 border-t border-seam bg-surface px-3 pb-[calc(0.5rem+env(safe-area-inset-bottom))] pt-2 md:hidden">
      <div className="flex min-w-0 flex-[0_1_auto] items-center gap-2 overflow-x-auto">
        {spaces.map((space) => (
          <button
            key={space.id}
            type="button"
            title={space.name}
            onClick={() => onSelectSpace(space)}
            className={`grid size-9 shrink-0 place-items-center rounded-xl text-sm font-semibold transition ${activeSpace?.id === space.id ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}
          >
            {space.name.slice(0, 1).toUpperCase()}
          </button>
        ))}
      </div>
      {canCreateSpace ? (
        <div className="shrink-0 border-l border-seam pl-2">
          <button type="button" aria-label="Add space" onClick={onCreateSpace} className="grid size-9 place-items-center rounded-xl text-muted hover:bg-[var(--ng-hover)] hover:text-text">
            <Plus size={16} />
          </button>
        </div>
      ) : null}
      <div className="ml-auto flex shrink-0 items-center gap-1 border-l border-seam pl-2">
        <button type="button" aria-label="History" title={transferCount > 0 ? `${transferCount} file transfers need attention` : "History"} onClick={onOpenHistory} className="relative grid size-9 place-items-center rounded-xl text-muted hover:bg-[var(--ng-hover)] hover:text-text">
          <History size={16} />
          {transferCount > 0 ? <span className="absolute right-0 top-0 min-w-4 rounded-full bg-primary px-1 text-center text-[10px] font-semibold leading-4 text-white" aria-hidden="true">{transferCount}</span> : null}
        </button>
        <button type="button" aria-label="Settings" onClick={onOpenSettings} className="grid size-9 place-items-center rounded-xl text-muted hover:bg-[var(--ng-hover)] hover:text-text">
          <Settings size={16} />
        </button>
      </div>
    </nav>
  );
}
