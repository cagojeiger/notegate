import { useState } from "react";
import { Plus, Settings } from "lucide-react";

import type { Space } from "../../api/types";
import { reorderSpacesByDrop } from "./spaceReorder";

type DropPosition = "before" | "after";

type DragTarget = {
  spaceId: string;
  position: DropPosition;
};

type ActivityRailProps = {
  spaces: Space[];
  activeSpace: Space | null;
  onSelectSpace: (space: Space) => void;
  onReorderSpaces: (spaces: Space[]) => void;
  onCreateSpace: () => void;
  onOpenSettings: () => void;
};

export function ActivityRail({ spaces, activeSpace, onSelectSpace, onReorderSpaces, onCreateSpace, onOpenSettings }: ActivityRailProps) {
  const [draggedSpaceId, setDraggedSpaceId] = useState<string | null>(null);
  const [dragTarget, setDragTarget] = useState<DragTarget | null>(null);
  const canReorder = spaces.length > 1;

  function clearDrag() {
    setDraggedSpaceId(null);
    setDragTarget(null);
  }

  return (
    <aside className="hidden w-[52px] shrink-0 min-h-0 flex-col border-r border-seam bg-surface md:flex">
      <div className="flex min-h-0 flex-1 flex-col items-center gap-2 overflow-y-auto py-3">
        {spaces.map((space) => {
          const active = activeSpace?.id === space.id;
          const dragging = draggedSpaceId === space.id;
          const dropBefore = dragTarget?.spaceId === space.id && dragTarget.position === "before";
          const dropAfter = dragTarget?.spaceId === space.id && dragTarget.position === "after";
          return (
            <div
              key={space.id}
              draggable={canReorder}
              onDragStart={(event) => {
                event.dataTransfer.effectAllowed = "move";
                event.dataTransfer.setData("text/plain", space.id);
                setDraggedSpaceId(space.id);
              }}
              onDragOver={(event) => {
                if (!draggedSpaceId || draggedSpaceId === space.id) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = "move";
                const rect = event.currentTarget.getBoundingClientRect();
                const position = event.clientY > rect.top + rect.height / 2 ? "after" : "before";
                setDragTarget({ spaceId: space.id, position });
              }}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragTarget(null);
              }}
              onDrop={(event) => {
                event.preventDefault();
                const sourceId = event.dataTransfer.getData("text/plain") || draggedSpaceId;
                if (!sourceId) return;
                const position = dragTarget?.spaceId === space.id ? dragTarget.position : "before";
                const ordered = reorderSpacesByDrop(spaces, sourceId, space.id, position);
                if (ordered !== spaces) onReorderSpaces(ordered);
                clearDrag();
              }}
              onDragEnd={clearDrag}
              className={`relative flex w-full justify-center py-0.5 ${canReorder ? "cursor-grab active:cursor-grabbing" : ""} ${dragging ? "opacity-45" : ""}`}
            >
              {dropBefore ? <span className="absolute left-2 right-2 -top-1 h-0.5 rounded-full bg-primary shadow-[0_0_0_1px_var(--ng-bg)]" aria-hidden="true" /> : null}
              {active ? <span className="absolute left-0 top-2 bottom-2 w-[3px] rounded-r-full bg-primary" aria-hidden="true" /> : null}
              <button type="button" onClick={() => onSelectSpace(space)} title={`${space.name}${canReorder ? " · drag to reorder" : ""}`} className={`grid size-9 place-items-center rounded-full border text-sm font-semibold transition ${active ? "border-border-strong bg-primary text-primary-contrast shadow-[var(--ng-inset-shadow)]" : "border-border bg-panel text-muted hover:bg-panel-strong hover:text-text"}`}>
                {space.name.slice(0, 1).toUpperCase()}
              </button>
              {dropAfter ? <span className="absolute left-2 right-2 -bottom-1 h-0.5 rounded-full bg-primary shadow-[0_0_0_1px_var(--ng-bg)]" aria-hidden="true" /> : null}
            </div>
          );
        })}
        <button onClick={onCreateSpace} className="grid size-9 place-items-center rounded-full border border-dashed border-border text-muted hover:border-border-strong hover:text-text" aria-label="Add space">
          <Plus size={16} />
        </button>
      </div>
      <div className="border-t border-seam p-2">
        <button onClick={onOpenSettings} className="grid size-9 place-items-center rounded-full border border-border bg-panel text-muted hover:bg-panel-strong hover:text-text" aria-label="Settings">
          <Settings size={16} />
        </button>
      </div>
    </aside>
  );
}
