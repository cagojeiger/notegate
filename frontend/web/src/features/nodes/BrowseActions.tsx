import { ChevronsDownUp, FilePlus, FolderPlus, Mic, MoreHorizontal, Pencil, Plus, RefreshCw, Trash2, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { Space } from "../../api/types";
import { Card, IconButton, MenuButton } from "../../shared/ui";
import { useRefreshSpace } from "./useNodeQueries";

export function BrowseActions({
  activeSpace,
  canWriteActiveSpace,
  canManageActiveSpace,
  onCreateFolder,
  onCreateText,
  onRecordAudio,
  onFileSelected,
  onCollapseTree,
  onRenameSpace,
  onDeleteSpace
}: {
  activeSpace: Space;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onRecordAudio: () => void;
  onFileSelected: (file: File | null) => void;
  onCollapseTree: () => void;
  onRenameSpace: () => void;
  onDeleteSpace: () => void;
}) {
  const refreshSpace = useRefreshSpace();
  const [createOpen, setCreateOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);

  return (
    <div className="relative flex items-center gap-1">
      <IconButton label="Refresh from server" onClick={() => refreshSpace(activeSpace.id)}><RefreshCw size={14} /></IconButton>
      <IconButton label="Collapse all folders" onClick={onCollapseTree}><ChevronsDownUp size={14} /></IconButton>
      <IconButton
        label="Create"
        onClick={() => {
          setMoreOpen(false);
          setCreateOpen((open) => !open);
        }}
        disabled={!canWriteActiveSpace}
      >
        <Plus size={14} />
      </IconButton>
      <IconButton
        label="Manage space"
        onClick={() => {
          setCreateOpen(false);
          setMoreOpen((open) => !open);
        }}
        disabled={!canManageActiveSpace}
      >
        <MoreHorizontal size={14} />
      </IconButton>
      {createOpen && canWriteActiveSpace ? (
        <CreateMenu
          onCreateFolder={onCreateFolder}
          onCreateText={onCreateText}
          onRecordAudio={onRecordAudio}
          onFileSelected={onFileSelected}
          onClose={() => setCreateOpen(false)}
        />
      ) : null}
      {moreOpen && canManageActiveSpace ? (
        <BrowseMenu
          onRenameSpace={onRenameSpace}
          onDeleteSpace={onDeleteSpace}
          onClose={() => setMoreOpen(false)}
        />
      ) : null}
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

function CreateMenu({ onCreateFolder, onCreateText, onRecordAudio, onFileSelected, onClose }: { onCreateFolder: () => void; onCreateText: () => void; onRecordAudio: () => void; onFileSelected: (file: File | null) => void; onClose: () => void }) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  useMenuDismiss(onClose);
  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <>
      <MenuBackdrop onClose={onClose} />
      <Card className="absolute right-0 top-full z-20 w-44 p-1 text-workbench shadow-[var(--ng-focus-shadow)]" padding="none">
        <MenuButton onClick={() => run(onCreateFolder)}><FolderPlus size={14} /> New folder</MenuButton>
        <MenuButton onClick={() => run(onCreateText)}><FilePlus size={14} /> New document</MenuButton>
        <MenuButton onClick={() => run(onRecordAudio)}><Mic size={14} /> Record audio</MenuButton>
        <MenuButton onClick={() => fileInputRef.current?.click()}><Upload size={14} /> Upload file</MenuButton>
        <input
          ref={fileInputRef}
          className="hidden"
          type="file"
          onChange={(event) => {
            onFileSelected(event.target.files?.[0] ?? null);
            onClose();
          }}
        />
      </Card>
    </>
  );
}

function BrowseMenu({ onRenameSpace, onDeleteSpace, onClose }: { onRenameSpace: () => void; onDeleteSpace: () => void; onClose: () => void }) {
  useMenuDismiss(onClose);
  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <>
      <MenuBackdrop onClose={onClose} />
      <Card className="absolute right-0 top-full z-20 w-44 p-1 text-workbench shadow-[var(--ng-focus-shadow)]" padding="none">
        <MenuButton onClick={() => run(onRenameSpace)}><Pencil size={14} /> Rename space</MenuButton>
        <MenuButton danger onClick={() => run(onDeleteSpace)}><Trash2 size={14} /> Delete space</MenuButton>
      </Card>
    </>
  );
}
