import type { ReactNode } from "react";

export function MenuButton({ children, onClick, danger, disabled }: { children: ReactNode; onClick: () => void; danger?: boolean; disabled?: boolean }) {
  return (
    <button type="button" onClick={onClick} disabled={disabled} className={`flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left hover:bg-panel disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent ${danger ? "text-danger" : "text-muted hover:text-text"}`}>
      {children}
    </button>
  );
}
