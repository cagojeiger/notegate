import type { HTMLAttributes } from "react";

export function PanelHeader({ children, className = "", tabs = false, active = false, ...props }: HTMLAttributes<HTMLDivElement> & { tabs?: boolean; active?: boolean }) {
  return (
    <div
      {...props}
      data-panel-header
      className={`flex h-workbench-header min-w-0 shrink-0 justify-between gap-2 border-b px-2 font-ui text-workbench ${tabs ? "items-end" : "items-center"} ${active ? "border-[var(--ng-active-border)] bg-[var(--ng-active-surface)]" : "border-seam"} ${className}`}
    >
      {children}
    </div>
  );
}
