import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export function Badge({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cn("rounded-full border border-border px-2 py-0.5 text-xs capitalize text-muted", className)}>{children}</span>;
}
