import { MoreHorizontal, Trash2 } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";

import { AnchoredOverlay, Card, IconButton, MenuButton } from "../../shared/ui";

type SupplementalAction = {
  label: string;
  icon?: ReactNode;
  onClick: () => void;
  disabled?: boolean;
};

export function NodeActionMenu({ onRenameNode, onMoveNode, onDeleteNode, disabled, supplementalActions = [] }: { onRenameNode: () => void; onMoveNode: () => void; onDeleteNode: () => void; disabled: boolean; supplementalActions?: SupplementalAction[] }) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);
  const overlayId = useId();
  const closeMenu = useCallback(() => setOpen(false), []);
  const hasEnabledAction = !disabled || supplementalActions.some((action) => !action.disabled);
  const menuOpen = open && hasEnabledAction;

  function run(action: () => void) {
    action();
    closeMenu();
  }

  useEffect(() => {
    if (!hasEnabledAction) closeMenu();
  }, [closeMenu, hasEnabledAction]);

  return (
    <div ref={anchorRef} className="relative">
      <IconButton
        label="More actions"
        expanded={menuOpen}
        controls={menuOpen ? overlayId : undefined}
        hasPopup="dialog"
        onClick={() => setOpen((value) => !value)}
        disabled={!hasEnabledAction}
      >
        <MoreHorizontal size={16} />
      </IconButton>
      <AnchoredOverlay
        anchorRef={anchorRef}
        open={menuOpen}
        onClose={closeMenu}
        id={overlayId}
        label="More actions"
        role="dialog"
        width={160}
        estimatedHeight={120 + supplementalActions.length * 36}
      >
        <Card className="w-full p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none">
          {supplementalActions.map((action) => (
            <MenuButton key={action.label} onClick={() => run(action.onClick)} disabled={action.disabled}>
              {action.icon}{action.label}
            </MenuButton>
          ))}
          {supplementalActions.length > 0 ? <div className="my-1 border-t border-border" /> : null}
          <MenuButton onClick={() => run(onRenameNode)} disabled={disabled}>Rename</MenuButton>
          <MenuButton onClick={() => run(onMoveNode)} disabled={disabled}>Move</MenuButton>
          <MenuButton danger onClick={() => run(onDeleteNode)} disabled={disabled}><Trash2 size={14} /> Delete</MenuButton>
        </Card>
      </AnchoredOverlay>
    </div>
  );
}
