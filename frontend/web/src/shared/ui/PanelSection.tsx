import type { HTMLAttributes } from "react";

export function PanelSection({ children, className = "", ...props }: HTMLAttributes<HTMLElement>) {
  return <section {...props} className={`border-b border-seam p-3 ${className}`}>{children}</section>;
}
