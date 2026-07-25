import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";

import { BrandAppIcon } from "../shared/ui";

type FullScreenStatusVariant = "loading" | "status";

export function FullScreenStatus({
  label,
  detail,
  action,
  variant = "loading"
}: {
  label: string;
  detail?: string;
  action?: ReactNode;
  variant?: FullScreenStatusVariant;
}) {
  return (
    <main className="grid h-full place-items-center bg-bg text-text">
      <div className="min-w-64 rounded-2xl border border-border bg-surface p-6 text-center shadow-[var(--ng-focus-shadow)]">
        <BrandAppIcon size={36} decorative className="mx-auto mb-4" />
        {variant === "loading" ? (
          <Loader2 className="mx-auto mb-3 animate-spin text-primary" size={20} aria-hidden="true" />
        ) : null}
        <div className="font-semibold">{label}</div>
        {detail ? <div className="mt-2 max-w-md text-sm text-muted">{detail}</div> : null}
        {action ? <div className="mt-4">{action}</div> : null}
      </div>
    </main>
  );
}
