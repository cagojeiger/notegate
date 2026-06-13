import { Copy, FilePlus, FolderPlus, Pencil, Trash2, Upload } from "lucide-react";
import { useEffect } from "react";

import type { RestNode } from "../../api/types";
import { Card, MenuButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

export function NodeContextMenu({ menu, onClose, onOpenNode, onRenameNode, onDeleteNode, onCreateInFolder, onUploadInFolder }: { menu: { x: number; y: number; node: RestNode }; onClose: () => void; onOpenNode: (node: RestNode) => void; onRenameNode: (node: RestNode) => void; onDeleteNode: (node: RestNode) => void; onCreateInFolder: (folder: RestNode, kind: "folder" | "text") => void; onUploadInFolder: (folder: RestNode, file: File | null) => void }) {
  const showToast = useUiStore((state) => state.showToast);
  const { node } = menu;
  const isRoot = node.parent_id === null;
  const isFolder = node.kind === "folder";
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
  function copyPath() {
    void navigator.clipboard?.writeText(node.path);
    showToast("Path copied");
  }
  const left = Math.min(menu.x, window.innerWidth - 196);
  const top = Math.min(menu.y, window.innerHeight - (isFolder ? 232 : 176));
  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <Card className="fixed z-50 w-48 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none" style={{ left, top }} role="menu">
        <div className="truncate px-3 py-1 text-xs text-muted">{node.path}</div>
        {isFolder ? (
          <>
            <MenuButton onClick={() => run(() => onCreateInFolder(node, "folder"))}><FolderPlus size={14} /> New folder</MenuButton>
            <MenuButton onClick={() => run(() => onCreateInFolder(node, "text"))}><FilePlus size={14} /> New text</MenuButton>
            <label className="flex cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-muted hover:bg-panel hover:text-text">
              <Upload size={14} /> Upload file
              <input className="hidden" type="file" onChange={(event) => { const file = event.target.files?.[0] ?? null; onClose(); onUploadInFolder(node, file); }} />
            </label>
            <div className="my-1 border-t border-border" />
          </>
        ) : null}
        <MenuButton onClick={() => run(() => onOpenNode(node))}>Open</MenuButton>
        <MenuButton onClick={() => run(() => onRenameNode(node))} disabled={isRoot}><Pencil size={14} /> Rename</MenuButton>
        <MenuButton onClick={() => run(copyPath)}><Copy size={14} /> Copy path</MenuButton>
        <MenuButton danger onClick={() => run(() => onDeleteNode(node))} disabled={isRoot}><Trash2 size={14} /> Delete</MenuButton>
      </Card>
    </>
  );
}
