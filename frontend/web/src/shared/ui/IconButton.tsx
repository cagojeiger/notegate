import type { ReactNode } from "react";

export function IconButton({ label, onClick, pressed, disabled, size = "md", children }: { label: string; onClick?: () => void; pressed?: boolean; disabled?: boolean; size?: "sm" | "md"; children: ReactNode }) {
  const sizeClass = size === "sm" ? "size-7 rounded-lg" : "size-8 rounded-[10px]";
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      onClick={onClick}
      disabled={disabled}
      className={`grid place-items-center text-muted transition hover:bg-[var(--ng-hover)] hover:text-text disabled:cursor-not-allowed disabled:opacity-40 ${sizeClass} ${pressed ? "bg-panel-strong text-text" : ""}`}
    >
      {children}
    </button>
  );
}
