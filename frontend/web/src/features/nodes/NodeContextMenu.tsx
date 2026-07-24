import { Copy, Download, FilePlus, FolderPlus, Move, PanelRightOpen, Pencil, Trash2, Upload, X } from "lucide-react";
import { useEffect } from "react";

import type { RestNode } from "../../entities/node/model";
import { copyText } from "../../shared/lib/clipboard";
import { Card, MenuButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

export function NodeContextMenu({
  menu,
  canWriteActiveSpace,
  canOpenInNewGroup = false,
  showCreateActions = true,
  onClose,
  onOpenNode,
  onOpenInNewGroup,
  onCloseGroup,
  onDownloadFile,
  onRenameNode,
  onMoveNode,
  onDeleteNode,
  onCreateInFolder,
  onUploadInFolder
}: {
  menu: { x: number; y: number; node: RestNode };
  canWriteActiveSpace: boolean;
  canOpenInNewGroup?: boolean;
  showCreateActions?: boolean;
  onClose: () => void;
  onOpenNode: (node: RestNode) => void;
  onOpenInNewGroup?: (node: RestNode) => void;
  onCloseGroup?: () => void;
  onDownloadFile?: (node: RestNode) => void;
  onRenameNode: (node: RestNode) => void;
  onMoveNode: (node: RestNode) => void;
  onDeleteNode: (node: RestNode) => void;
  onCreateInFolder: (folder: RestNode, kind: "folder" | "text") => void;
  onUploadInFolder: (folder: RestNode, file: File | null) => void;
}) {
  const showToast = useUiStore((state) => state.showToast);
  const { node } = menu;
  const isRoot = node.parent_id === null;
  const isFolder = node.kind === "folder";
  const canMutateNode = !isRoot && canWriteActiveSpace;
  const canCreateInNode = showCreateActions && isFolder && canWriteActiveSpace;

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

  async function copyPath() {
    showToast((await copyText(node.path)) ? "Path copied" : "Could not copy path");
  }

  const maxHeight = canCreateInNode ? 304 : 236;
  const left = Math.min(menu.x, window.innerWidth - 216);
  const top = Math.min(menu.y, window.innerHeight - maxHeight);

  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <Card className="fixed z-50 w-52 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none" style={{ left, top }} role="menu">
        <div className="truncate px-3 py-1 text-xs text-muted">{node.path}</div>
        {canCreateInNode ? (
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
        {onOpenInNewGroup ? <MenuButton onClick={() => run(() => onOpenInNewGroup(node))} disabled={!canOpenInNewGroup || isRoot}><PanelRightOpen size={14} /> Open in new group</MenuButton> : null}
        {onDownloadFile && node.kind === "file" ? <MenuButton onClick={() => run(() => onDownloadFile(node))}><Download size={14} /> Download</MenuButton> : null}
        <MenuButton onClick={() => run(() => onRenameNode(node))} disabled={!canMutateNode}><Pencil size={14} /> Rename</MenuButton>
        <MenuButton onClick={() => run(() => onMoveNode(node))} disabled={!canMutateNode}><Move size={14} /> Move…</MenuButton>
        <MenuButton onClick={() => run(() => { void copyPath(); })}><Copy size={14} /> Copy path</MenuButton>
        {onCloseGroup ? <MenuButton onClick={() => run(onCloseGroup)}><X size={14} /> Close group</MenuButton> : null}
        <MenuButton danger onClick={() => run(() => onDeleteNode(node))} disabled={!canMutateNode}><Trash2 size={14} /> Delete</MenuButton>
      </Card>
    </>
  );
}
