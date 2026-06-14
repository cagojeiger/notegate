import type { ReactNode } from "react";

export function MenuButton({ children, onClick, danger, disabled }: { children: ReactNode; onClick: () => void; danger?: boolean; disabled?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left outline-none transition focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent ${
        danger ? "text-danger hover:bg-danger/10" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"
      }`}
    >
      {children}
    </button>
  );
}
