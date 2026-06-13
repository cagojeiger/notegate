import { MoreHorizontal, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { Card, IconButton, MenuButton } from "../../shared/ui";

export function NodeActionMenu({ onRenameNode, onMoveNode, onDeleteNode, disabled }: { onRenameNode: () => void; onMoveNode: () => void; onDeleteNode: () => void; disabled: boolean }) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);
  return (
    <div className="relative">
      <IconButton label="Node actions" onClick={() => setOpen((value) => !value)} disabled={disabled}><MoreHorizontal size={16} /></IconButton>
      {open ? (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} onContextMenu={(event) => { event.preventDefault(); setOpen(false); }} aria-hidden="true" />
          <Card className="absolute right-0 top-9 z-20 w-40 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none">
            <MenuButton onClick={() => { onRenameNode(); setOpen(false); }}>Rename</MenuButton>
            <MenuButton onClick={() => { onMoveNode(); setOpen(false); }}>Move</MenuButton>
            <MenuButton danger onClick={() => { onDeleteNode(); setOpen(false); }}><Trash2 size={14} /> Delete</MenuButton>
          </Card>
        </>
      ) : null}
    </div>
  );
}
