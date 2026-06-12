import type { ReactNode } from "react";

export function MenuButton({ children, onClick, danger }: { children: ReactNode; onClick: () => void; danger?: boolean }) {
  return (
    <button type="button" onClick={onClick} className={`flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left hover:bg-panel ${danger ? "text-danger" : "text-muted hover:text-text"}`}>
      {children}
    </button>
  );
}
