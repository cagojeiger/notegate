import type { ReactNode } from "react";

export function Button({ children, onClick, secondary, disabled }: { children: ReactNode; onClick?: () => void; secondary?: boolean; disabled?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={
        secondary
          ? "rounded-lg border border-border bg-surface px-3 py-2 text-sm text-muted hover:bg-panel hover:text-text disabled:opacity-50"
          : "rounded-lg bg-primary px-3 py-2 text-sm font-semibold text-primary-contrast shadow-[var(--ng-inset-shadow)] hover:bg-[var(--ng-primary-hover)] disabled:opacity-50"
      }
    >
      {children}
    </button>
  );
}
