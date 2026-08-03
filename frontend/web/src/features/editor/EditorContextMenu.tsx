import { ChevronsDownUp, ChevronsUpDown, Copy, Move, PanelRightOpen, Pencil, Save, Trash2, Undo2, X } from "lucide-react";
import { useEffect } from "react";

import type { RestNode } from "../../api/types";
import { Card, MenuButton } from "../../shared/ui";

export default function EditorContextMenu({
  menu,
  node,
  mode,
  canCopyContent,
  canCopyPath,
  canEditText,
  canSave,
  canMutateNode,
  canOpenInNewGroup,
  canCloseGroup,
  showStructuredActions,
  structuredActionsDisabled,
  onClose,
  onCopyContent,
  onEditText,
  onSaveDraft,
  onCancelEdit,
  onOpenInNewGroup,
  onCopyPath,
  onCloseGroup,
  onExpandAll,
  onCollapseAll,
  onRenameNode,
  onMoveNode,
  onDeleteNode
}: {
  menu: { x: number; y: number };
  node: RestNode;
  mode: "preview" | "edit";
  canCopyContent: boolean;
  canCopyPath: boolean;
  canEditText: boolean;
  canSave: boolean;
  canMutateNode: boolean;
  canOpenInNewGroup: boolean;
  canCloseGroup: boolean;
  showStructuredActions: boolean;
  structuredActionsDisabled: boolean;
  onClose: () => void;
  onCopyContent: () => void;
  onEditText: () => void;
  onSaveDraft: () => void;
  onCancelEdit: () => void;
  onOpenInNewGroup: () => void;
  onCopyPath: () => void;
  onCloseGroup: () => void;
  onExpandAll: () => void;
  onCollapseAll: () => void;
  onRenameNode: () => void;
  onMoveNode: () => void;
  onDeleteNode: () => void;
}) {
  const menuWidth = 208;
  const menuHeight = (mode === "edit" ? 332 : 296) + (showStructuredActions ? 64 : 0);

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

  const left = Math.max(8, Math.min(menu.x, window.innerWidth - menuWidth - 8));
  const top = Math.max(8, Math.min(menu.y, window.innerHeight - menuHeight - 8));
  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <Card className="fixed z-50 w-52 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none" style={{ left, top }} role="menu" aria-label="Editor actions">
        <div className="truncate px-3 py-1 text-xs text-muted">{node.name}</div>
        <MenuButton onClick={() => run(onCopyContent)} disabled={!canCopyContent}><Copy size={14} /> Copy content</MenuButton>
        {showStructuredActions ? (
          <>
            <MenuButton onClick={() => run(onExpandAll)} disabled={structuredActionsDisabled}><ChevronsUpDown size={14} /> Expand all</MenuButton>
            <MenuButton onClick={() => run(onCollapseAll)} disabled={structuredActionsDisabled}><ChevronsDownUp size={14} /> Collapse all</MenuButton>
          </>
        ) : null}
        {mode === "edit" ? (
          <>
            <MenuButton onClick={() => run(onSaveDraft)} disabled={!canSave}><Save size={14} /> Save</MenuButton>
            <MenuButton onClick={() => run(onCancelEdit)}><Undo2 size={14} /> Cancel edit</MenuButton>
          </>
        ) : (
          <MenuButton onClick={() => run(onEditText)} disabled={!canEditText}><Pencil size={14} /> Edit</MenuButton>
        )}
        <MenuButton onClick={() => run(onOpenInNewGroup)} disabled={!canOpenInNewGroup}><PanelRightOpen size={14} /> Open in new group</MenuButton>
        <MenuButton onClick={() => run(onCopyPath)} disabled={!canCopyPath}><Copy size={14} /> Copy path</MenuButton>
        {canCloseGroup ? <MenuButton onClick={() => run(onCloseGroup)}><X size={14} /> Close group</MenuButton> : null}
        <div className="my-1 border-t border-border" />
        <MenuButton onClick={() => run(onRenameNode)} disabled={!canMutateNode}><Pencil size={14} /> Rename</MenuButton>
        <MenuButton onClick={() => run(onMoveNode)} disabled={!canMutateNode}><Move size={14} /> Move…</MenuButton>
        <MenuButton danger onClick={() => run(onDeleteNode)} disabled={!canMutateNode}><Trash2 size={14} /> Delete</MenuButton>
      </Card>
    </>
  );
}
