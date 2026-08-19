import type { ButtonHTMLAttributes, ReactNode } from "react";

import { cn } from "../lib/cn";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type ButtonSize = "xs" | "sm" | "md";

export function Button({
  children,
  secondary,
  variant = secondary ? "secondary" : "primary",
  size = "md",
  className,
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  children: ReactNode;
  secondary?: boolean;
  variant?: ButtonVariant;
  size?: ButtonSize;
}) {
  const sizeClass = size === "xs"
    ? "min-h-workbench-control px-2 text-workbench md:min-h-6"
    : size === "sm"
      ? "min-h-workbench-control px-2 text-workbench"
      : "min-h-workbench-control px-2.5 text-workbench";
  const variantClass = {
    primary: "bg-primary text-primary-contrast shadow-[var(--ng-inset-shadow)] hover:bg-[var(--ng-primary-hover)]",
    secondary: "border border-border-strong bg-surface text-muted hover:bg-[var(--ng-hover)] hover:text-text",
    ghost: "text-muted hover:bg-[var(--ng-hover)] hover:text-text",
    danger: "border border-danger/30 text-danger hover:bg-danger/10"
  }[variant];

  return (
    <button
      type={type}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-workbench-surface font-medium outline-none transition focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-50",
        sizeClass,
        variantClass,
        className
      )}
      {...props}
    >
      {children}
    </button>
  );
}
