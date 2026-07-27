import { MoreHorizontal, Trash2 } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState } from "react";

import { AnchoredOverlay, Card, IconButton, MenuButton } from "../../shared/ui";

export function NodeActionMenu({ onRenameNode, onMoveNode, onDeleteNode, disabled }: { onRenameNode: () => void; onMoveNode: () => void; onDeleteNode: () => void; disabled: boolean }) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);
  const overlayId = useId();
  const closeMenu = useCallback(() => setOpen(false), []);
  const menuOpen = open && !disabled;

  function run(action: () => void) {
    action();
    closeMenu();
  }

  useEffect(() => {
    if (disabled) closeMenu();
  }, [closeMenu, disabled]);

  return (
    <div ref={anchorRef} className="relative">
      <IconButton
        label="Node actions"
        expanded={menuOpen}
        controls={menuOpen ? overlayId : undefined}
        hasPopup="dialog"
        onClick={() => setOpen((value) => !value)}
        disabled={disabled}
      >
        <MoreHorizontal size={16} />
      </IconButton>
      <AnchoredOverlay
        anchorRef={anchorRef}
        open={menuOpen}
        onClose={closeMenu}
        id={overlayId}
        label="Node actions"
        role="dialog"
        width={160}
        estimatedHeight={120}
      >
        <Card className="w-full p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none">
          <MenuButton onClick={() => run(onRenameNode)} disabled={disabled}>Rename</MenuButton>
          <MenuButton onClick={() => run(onMoveNode)} disabled={disabled}>Move</MenuButton>
          <MenuButton danger onClick={() => run(onDeleteNode)} disabled={disabled}><Trash2 size={14} /> Delete</MenuButton>
        </Card>
      </AnchoredOverlay>
    </div>
  );
}
