import { useEffect, useState } from "react";
import { Copy, History, Pencil, Plus, Settings, Trash2 } from "lucide-react";

import type { Space } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import { Card, MenuButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { reorderSpacesByDrop } from "./spaceReorder";

type DropPosition = "before" | "after";

type DragTarget = {
  spaceId: string;
  position: DropPosition;
};

type ActivityRailProps = {
  spaces: Space[];
  activeSpace: Space | null;
  canCreateSpace: boolean;
  canManageSpaces: boolean;
  transferCount: number;
  onSelectSpace: (space: Space) => void;
  onReorderSpaces: (spaces: Space[]) => void;
  onCreateSpace: () => void;
  onRenameSpace: (space: Space) => void;
  onDeleteSpace: (space: Space) => void;
  onOpenHistory: () => void;
  onOpenSettings: () => void;
};

export function ActivityRail({ spaces, activeSpace, canCreateSpace, canManageSpaces, transferCount, onSelectSpace, onReorderSpaces, onCreateSpace, onRenameSpace, onDeleteSpace, onOpenHistory, onOpenSettings }: ActivityRailProps) {
  const [draggedSpaceId, setDraggedSpaceId] = useState<string | null>(null);
  const [dragTarget, setDragTarget] = useState<DragTarget | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; space: Space } | null>(null);
  const canReorder = canManageSpaces && spaces.length > 1;

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
              onContextMenu={(event) => {
                event.preventDefault();
                setMenu({ x: event.clientX, y: event.clientY, space });
              }}
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
              <button type="button" onClick={() => onSelectSpace(space)} title={`${space.name}${canReorder ? " · drag to reorder" : ""}`} className={`grid size-9 place-items-center rounded-xl text-sm font-semibold transition ${active ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}>
                {space.name.slice(0, 1).toUpperCase()}
              </button>
              {dropAfter ? <span className="absolute left-2 right-2 -bottom-1 h-0.5 rounded-full bg-primary shadow-[0_0_0_1px_var(--ng-bg)]" aria-hidden="true" /> : null}
            </div>
          );
        })}
        {canCreateSpace ? (
          <button onClick={onCreateSpace} className="grid size-9 place-items-center rounded-xl text-muted transition hover:bg-[var(--ng-hover)] hover:text-text" aria-label="Add space">
            <Plus size={16} />
          </button>
        ) : null}
      </div>
      <div className="space-y-1 border-t border-seam p-2">
        <button onClick={onOpenHistory} className="relative grid size-9 place-items-center rounded-xl text-muted transition hover:bg-[var(--ng-hover)] hover:text-text" aria-label="History" title={transferCount > 0 ? `${transferCount} file transfers need attention` : "History"}>
          <History size={16} />
          {transferCount > 0 ? <span className="absolute right-0 top-0 min-w-4 rounded-full bg-primary px-1 text-center text-[10px] font-semibold leading-4 text-white" aria-hidden="true">{transferCount}</span> : null}
        </button>
        <button onClick={onOpenSettings} className="grid size-9 place-items-center rounded-xl text-muted transition hover:bg-[var(--ng-hover)] hover:text-text" aria-label="Settings">
          <Settings size={16} />
        </button>
      </div>
      {menu ? <SpaceContextMenu menu={menu} canManageSpaces={canManageSpaces} onClose={() => setMenu(null)} onSelectSpace={onSelectSpace} onRenameSpace={onRenameSpace} onDeleteSpace={onDeleteSpace} /> : null}
    </aside>
  );
}

function SpaceContextMenu({ menu, canManageSpaces, onClose, onSelectSpace, onRenameSpace, onDeleteSpace }: { menu: { x: number; y: number; space: Space }; canManageSpaces: boolean; onClose: () => void; onSelectSpace: (space: Space) => void; onRenameSpace: (space: Space) => void; onDeleteSpace: (space: Space) => void }) {
  const showToast = useUiStore((state) => state.showToast);
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function run(action: () => void) {
    action();
    onClose();
  }

  const canManageSpace = canManageSpaces && menu.space.permission === "write";

  async function copySpaceId() {
    showToast((await copyText(menu.space.id)) ? "Space id copied" : "Could not copy space id");
  }

  const left = Math.min(menu.x, window.innerWidth - 196);
  const top = Math.min(menu.y, window.innerHeight - 184);
  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <Card className="fixed z-50 w-48 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none" style={{ left, top }} role="menu">
        <div className="truncate px-3 py-1 text-xs text-muted">{menu.space.name}</div>
        <MenuButton onClick={() => run(() => onSelectSpace(menu.space))}>Select</MenuButton>
        <MenuButton onClick={() => run(() => onRenameSpace(menu.space))} disabled={!canManageSpace}><Pencil size={14} /> Rename</MenuButton>
        <MenuButton onClick={() => run(() => { void copySpaceId(); })}><Copy size={14} /> Copy space id</MenuButton>
        <MenuButton danger onClick={() => run(() => onDeleteSpace(menu.space))} disabled={!canManageSpace}><Trash2 size={14} /> Delete</MenuButton>
      </Card>
    </>
  );
}
