import type { ButtonHTMLAttributes, Ref } from "react";
import { Plus } from "lucide-react";

import { cn } from "../lib/cn";

export function AddButton({ label, layout = "row", className, type = "button", ...props }:
  Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-label"> & {
    label: string;
    layout?: "row" | "card";
    ref?: Ref<HTMLButtonElement>;
  }) {
  return <button {...props} type={type} aria-label={label}
    className={cn(
      "inline-flex w-full items-center justify-center gap-1.5 rounded-workbench-surface border border-dashed border-border-strong bg-surface font-ui text-workbench font-medium text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-50",
      layout === "card" ? "h-full min-h-36" : "h-12 min-h-workbench-control",
      className
    )}>
    <Plus size={layout === "card" ? 28 : 17} aria-hidden="true" />
    {layout === "row" ? label : null}
  </button>;
}
