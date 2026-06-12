import type { ReactNode } from "react";

export function IconButton({ label, onClick, pressed, disabled, children }: { label: string; onClick?: () => void; pressed?: boolean; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      onClick={onClick}
      disabled={disabled}
      className={`grid size-8 place-items-center rounded-lg border border-border bg-panel text-muted transition hover:bg-panel-strong hover:text-text disabled:cursor-not-allowed disabled:opacity-40 ${pressed ? "bg-panel-strong text-text" : ""}`}
    >
      {children}
    </button>
  );
}
