import type { ReactNode } from "react";

import { HelpTooltip } from "./HelpTooltip";

export function SectionHeader({ title, description, help, actions }: { title: string; description?: ReactNode; help?: string; actions?: ReactNode }) {
  return (
    <div className="mb-2 flex items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="relative flex items-center gap-1">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">{title}</h3>
          {help ? (
            <HelpTooltip label={`About ${title}`}>{help}</HelpTooltip>
          ) : null}
        </div>
        {description ? <p className="mt-1 text-xs text-muted">{description}</p> : null}
      </div>
      {actions ? <div className="shrink-0">{actions}</div> : null}
    </div>
  );
}
