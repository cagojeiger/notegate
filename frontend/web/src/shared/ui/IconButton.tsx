import type { AriaAttributes, ReactNode } from "react";

type IconButtonProps = {
  label: string;
  onClick?: () => void;
  pressed?: boolean;
  expanded?: boolean;
  controls?: string;
  hasPopup?: AriaAttributes["aria-haspopup"];
  disabled?: boolean;
  autoFocus?: boolean;
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
  autoFocus,
  size = "md",
  children
}: IconButtonProps) {
  const sizeClass = size === "sm"
    ? "size-workbench-control rounded-workbench"
    : "size-workbench-control rounded-workbench-surface";
  const active = pressed === true || expanded === true;
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={pressed}
      aria-expanded={expanded}
      aria-controls={controls}
      aria-haspopup={hasPopup}
      onClick={onClick}
      disabled={disabled}
      autoFocus={autoFocus}
      className={`grid shrink-0 place-items-center text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text disabled:cursor-not-allowed disabled:opacity-40 ${sizeClass} ${active ? "bg-panel-strong text-text" : ""}`}
    >
      {children}
    </button>
  );
}
