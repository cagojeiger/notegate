import { Plus, Settings } from "lucide-react";

import type { Space } from "../../api/types";

// Mobile presentation of the ActivityRail: a bottom space switcher bar.
// Space list scrolls; ＋ hugs the list end; Settings is pinned far-right (docs/ui 02-layout).
export function MobileSpaceBar({ spaces, activeSpace, onSelectSpace, onCreateSpace, onOpenSettings }: { spaces: Space[]; activeSpace: Space | null; onSelectSpace: (space: Space) => void; onCreateSpace: () => void; onOpenSettings: () => void }) {
  return (
    <nav aria-label="Spaces" className="flex items-center gap-2 border-t border-seam bg-surface px-3 py-2 md:hidden">
      <div className="flex min-w-0 flex-[0_1_auto] items-center gap-2 overflow-x-auto">
        {spaces.map((space) => (
          <button
            key={space.id}
            type="button"
            title={space.name}
            onClick={() => onSelectSpace(space)}
            className={`grid size-9 shrink-0 place-items-center rounded-full border text-sm font-semibold transition ${activeSpace?.id === space.id ? "border-border-strong bg-primary text-primary-contrast shadow-[var(--ng-inset-shadow)]" : "border-border bg-panel text-muted"}`}
          >
            {space.name.slice(0, 1).toUpperCase()}
          </button>
        ))}
      </div>
      <div className="shrink-0 border-l border-seam pl-2">
        <button type="button" aria-label="Add space" onClick={onCreateSpace} className="grid size-9 place-items-center rounded-full border border-dashed border-border text-muted">
          <Plus size={16} />
        </button>
      </div>
      <div className="ml-auto shrink-0 border-l border-seam pl-2">
        <button type="button" aria-label="Settings" onClick={onOpenSettings} className="grid size-9 place-items-center rounded-full border border-border bg-panel text-muted">
          <Settings size={16} />
        </button>
      </div>
    </nav>
  );
}
