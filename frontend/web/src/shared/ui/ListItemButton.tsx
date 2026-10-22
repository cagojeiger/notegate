import type { ButtonHTMLAttributes, Ref } from "react";

import { cn } from "../lib/cn";

// Content owns alignment; selectable rows share their inset focus and left indicator.
export function ListItemButton({ selected, className, type = "button", ...props }:
  Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-pressed"> & { selected?: boolean; ref?: Ref<HTMLButtonElement> }) {
  return <button {...props} type={type} aria-pressed={selected}
    className={cn(
      "flex min-h-workbench-control w-full gap-3 border-l-2 px-3 py-3 text-left font-ui text-workbench outline-none transition disabled:cursor-not-allowed disabled:opacity-50",
      selected ? "border-[var(--ng-active-border)] bg-[var(--ng-selection)]" : "border-transparent hover:bg-[var(--ng-hover)]",
      className
    )} />;
}
