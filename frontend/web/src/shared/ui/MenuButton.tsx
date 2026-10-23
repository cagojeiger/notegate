import { useContext, type ReactNode } from "react";

import { ContextMenuItems } from "./ContextMenu";

export function MenuButton({ children, onClick, danger, disabled }: { children: ReactNode; onClick: () => void; danger?: boolean; disabled?: boolean }) {
  const menuItem = useContext(ContextMenuItems);
  return (
    <button
      type="button"
      role={menuItem ? "menuitem" : undefined}
      tabIndex={menuItem ? -1 : undefined}
      onClick={onClick}
      disabled={disabled}
      className={`flex min-h-workbench-control w-full items-center gap-2 rounded-workbench px-2 text-left text-workbench outline-none transition disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent ${
        danger ? "text-danger hover:bg-danger/10" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"
      }`}
    >
      {children}
    </button>
  );
}
