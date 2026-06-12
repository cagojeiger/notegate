import { Plus, Settings } from "lucide-react";

import type { Space } from "../../api/types";

export function ActivityRail({ spaces, activeSpace, onSelectSpace, onCreateSpace, onSignOut }: { spaces: Space[]; activeSpace: Space | null; onSelectSpace: (space: Space) => void; onCreateSpace: () => void; onSignOut: () => void }) {
  return (
    <aside className="hidden w-[52px] shrink-0 min-h-0 flex-col border-r border-border bg-surface md:flex">
      <div className="flex min-h-0 flex-1 flex-col items-center gap-2 overflow-y-auto py-3">
        {spaces.map((space) => (
          <button key={space.id} onClick={() => onSelectSpace(space)} title={space.name} className={`grid size-9 place-items-center rounded-full border text-sm font-semibold transition ${activeSpace?.id === space.id ? "border-border-strong bg-primary text-primary-contrast shadow-[var(--ng-inset-shadow)]" : "border-border bg-panel text-muted hover:bg-panel-strong hover:text-text"}`}>
            {space.name.slice(0, 1).toUpperCase()}
          </button>
        ))}
        <button onClick={onCreateSpace} className="grid size-9 place-items-center rounded-full border border-dashed border-border text-muted hover:border-border-strong hover:text-text" aria-label="Add space">
          <Plus size={16} />
        </button>
      </div>
      <div className="border-t border-border p-2">
        <button onClick={onSignOut} className="grid size-9 place-items-center rounded-full border border-border bg-panel text-muted hover:bg-panel-strong hover:text-text" aria-label="Settings">
          <Settings size={16} />
        </button>
      </div>
    </aside>
  );
}
