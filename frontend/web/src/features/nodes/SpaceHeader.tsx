import { FileText, Folder, MoreHorizontal, Plus, RefreshCw, Trash2, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import type { Space } from "../../api/types";
import { Card, IconButton, MenuButton } from "../../shared/ui";
import { useRefreshSpace } from "./useNodeQueries";

export function SpaceHeader({ activeSpace, canWriteActiveSpace, canManageActiveSpace, onCreateFolder, onCreateText, onFileSelected, onRenameSpace, onDeleteSpace }: { activeSpace: Space | null; canWriteActiveSpace: boolean; canManageActiveSpace: boolean; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void; onRenameSpace: () => void; onDeleteSpace: () => void }) {
  const refreshSpace = useRefreshSpace();
  const [createOpen, setCreateOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const canRefresh = !!activeSpace;
  return (
    <div className="relative flex h-12 items-center justify-between border-b border-seam px-3">
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold">{activeSpace?.name ?? "No space"}</div>
        {activeSpace ? <div className="text-[10px] uppercase tracking-wide text-faint">active space</div> : null}
      </div>
      <div className="flex items-center gap-1">
        <IconButton label="Refresh from server" onClick={() => { if (activeSpace) refreshSpace(activeSpace.id); }} disabled={!canRefresh}><RefreshCw size={15} /></IconButton>
        <IconButton label="Create node" onClick={() => setCreateOpen((open) => !open)} disabled={!canWriteActiveSpace}><Plus size={15} /></IconButton>
        <IconButton label="Manage space" onClick={() => setManageOpen((open) => !open)} disabled={!canManageActiveSpace}><MoreHorizontal size={15} /></IconButton>
      </div>
      {createOpen && canWriteActiveSpace ? <CreateMenu onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} onClose={() => setCreateOpen(false)} /> : null}
      {manageOpen && canManageActiveSpace ? <SpaceMenu onRenameSpace={onRenameSpace} onDeleteSpace={onDeleteSpace} onClose={() => setManageOpen(false)} /> : null}
    </div>
  );
}

function useMenuDismiss(onClose: () => void) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
}

function MenuBackdrop({ onClose }: { onClose: () => void }) {
  return <div className="fixed inset-0 z-10" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} aria-hidden="true" />;
}

function CreateMenu({ onCreateFolder, onCreateText, onFileSelected, onClose }: { onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void; onClose: () => void }) {
  useMenuDismiss(onClose);
  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <>
      <MenuBackdrop onClose={onClose} />
      <Card className="absolute right-3 top-11 z-20 w-44 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none">
        <MenuButton onClick={() => run(onCreateFolder)}><Folder size={14} /> New folder</MenuButton>
        <MenuButton onClick={() => run(onCreateText)}><FileText size={14} /> New text</MenuButton>
        <label className="flex cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-muted hover:bg-panel hover:text-text">
          <Upload size={14} /> Upload file
          <input
            className="hidden"
            type="file"
            onChange={(event) => {
              onFileSelected(event.target.files?.[0] ?? null);
              onClose();
            }}
          />
        </label>
      </Card>
    </>
  );
}

function SpaceMenu({ onRenameSpace, onDeleteSpace, onClose }: { onRenameSpace: () => void; onDeleteSpace: () => void; onClose: () => void }) {
  useMenuDismiss(onClose);
  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <>
      <MenuBackdrop onClose={onClose} />
      <Card className="absolute right-3 top-11 z-20 w-44 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none">
        <MenuButton onClick={() => run(onRenameSpace)}>Rename space</MenuButton>
        <MenuButton danger onClick={() => run(onDeleteSpace)}><Trash2 size={14} /> Delete space</MenuButton>
      </Card>
    </>
  );
}
