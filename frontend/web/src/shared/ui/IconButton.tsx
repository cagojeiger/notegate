import type { AriaAttributes, ReactNode } from "react";

type IconButtonProps = {
  label: string;
  onClick?: () => void;
  pressed?: boolean;
  expanded?: boolean;
  controls?: string;
  hasPopup?: AriaAttributes["aria-haspopup"];
  disabled?: boolean;
  size?: "sm" | "md";
  children: ReactNode;
};

export function IconButton({
  label,
  onClick,
  pressed,
  expanded,
  controls,
  hasPopup,
  disabled,
  size = "md",
  children
}: IconButtonProps) {
  const sizeClass = size === "sm" ? "size-7 rounded-lg" : "size-8 rounded-[10px]";
  const active = pressed === true || expanded === true;
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      aria-expanded={expanded}
      aria-controls={controls}
      aria-haspopup={hasPopup}
      onClick={onClick}
      disabled={disabled}
      className={`grid place-items-center text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-40 ${sizeClass} ${active ? "bg-panel-strong text-text" : ""}`}
    >
      {children}
    </button>
  );
}
