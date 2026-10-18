import { ArrowRight, Copy, Download, FilePlus, FolderPlus, Move, PanelRightOpen, Pencil, Trash2, Upload, X } from "lucide-react";
import { useRef } from "react";

import type { NodeSummary } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import { MenuButton } from "../../shared/ui";
import { ContextMenu } from "../../shared/ui/ContextMenu";
import { useUiStore } from "../../stores/uiStore";
import { canCreateInFolder, canMutateNode } from "./nodeWriteAccess";

export function NodeContextMenu({
  menu,
  qualifiedPath,
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
  menu: { x: number; y: number; node: NodeSummary };
  qualifiedPath: string | null;
  canWriteActiveSpace: boolean;
  canOpenInNewGroup?: boolean;
  showCreateActions?: boolean;
  onClose: () => void;
  onOpenNode: (node: NodeSummary) => void;
  onOpenInNewGroup?: (node: NodeSummary) => void;
  onCloseGroup?: () => void;
  onDownloadFile?: (node: NodeSummary) => void;
  onRenameNode: (node: NodeSummary) => void;
  onMoveNode: (node: NodeSummary) => void;
  onDeleteNode: (node: NodeSummary) => void;
  onCreateInFolder: (folder: NodeSummary, kind: "folder" | "text") => void;
  onUploadInFolder: (folder: NodeSummary, file: File | null) => void;
}) {
  const showToast = useUiStore((state) => state.showToast);
  const uploadInputRef = useRef<HTMLInputElement>(null);
  const { node } = menu;
  const isRoot = node.parent_id === null;
  const isFolder = node.kind === "folder";
  const nodeCanBeMutated = canMutateNode(node, canWriteActiveSpace);
  const showCreateInNode = showCreateActions && isFolder && canWriteActiveSpace;
  const canCreateInNode = showCreateInNode && canCreateInFolder(node, canWriteActiveSpace);

  function run(action: () => void) {
    action();
    onClose();
  }

  async function copyPath() {
    if (!qualifiedPath) return;
    showToast((await copyText(qualifiedPath)) ? "Path copied" : "Could not copy path");
  }

  return (
    <ContextMenu x={menu.x} y={menu.y} label="File actions" onClose={onClose}>
      <div className="truncate px-3 py-1 text-xs text-muted">{qualifiedPath ?? node.path}</div>
      {showCreateInNode ? (
        <>
          <MenuButton onClick={() => run(() => onCreateInFolder(node, "folder"))} disabled={!canCreateInNode}><FolderPlus size={14} /> New folder</MenuButton>
          <MenuButton onClick={() => run(() => onCreateInFolder(node, "text"))} disabled={!canCreateInNode}><FilePlus size={14} /> New document</MenuButton>
          <MenuButton onClick={() => uploadInputRef.current?.click()} disabled={!canCreateInNode}>
            <Upload size={14} /> Upload file
          </MenuButton>
          <input ref={uploadInputRef} className="hidden" type="file" disabled={!canCreateInNode} onChange={(event) => { const file = event.target.files?.[0] ?? null; onClose(); onUploadInFolder(node, file); }} />
          <div className="my-1 border-t border-border" />
        </>
      ) : null}
      <MenuButton onClick={() => run(() => onOpenNode(node))}><ArrowRight size={14} /> Open</MenuButton>
      {onOpenInNewGroup ? <MenuButton onClick={() => run(() => onOpenInNewGroup(node))} disabled={!canOpenInNewGroup || isRoot}><PanelRightOpen size={14} /> Open in new group</MenuButton> : null}
      {onDownloadFile && node.kind === "file" ? <MenuButton onClick={() => run(() => onDownloadFile(node))}><Download size={14} /> Download</MenuButton> : null}
      <MenuButton onClick={() => run(() => onRenameNode(node))} disabled={!nodeCanBeMutated}><Pencil size={14} /> Rename</MenuButton>
      <MenuButton onClick={() => run(() => onMoveNode(node))} disabled={!nodeCanBeMutated}><Move size={14} /> Move…</MenuButton>
      <MenuButton onClick={() => run(() => { void copyPath(); })} disabled={!qualifiedPath}><Copy size={14} /> Copy path</MenuButton>
      {onCloseGroup ? <MenuButton onClick={() => run(onCloseGroup)}><X size={14} /> Close group</MenuButton> : null}
      <MenuButton danger onClick={() => run(() => onDeleteNode(node))} disabled={!nodeCanBeMutated}><Trash2 size={14} /> Delete</MenuButton>
    </ContextMenu>
  );
}
