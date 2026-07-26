import { useId, type ReactNode } from "react";
import { CircleHelp } from "lucide-react";

export function SectionHeader({ title, description, help, actions }: { title: string; description?: ReactNode; help?: string; actions?: ReactNode }) {
  const helpId = useId();

  return (
    <div className="mb-2 flex items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="relative flex items-center gap-1">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">{title}</h3>
          {help ? (
            <>
              <button
                type="button"
                className="peer inline-grid size-6 shrink-0 place-items-center rounded text-muted outline-none hover:bg-panel-strong hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45"
                aria-label={`About ${title}`}
                aria-describedby={helpId}
              >
                <CircleHelp size={13} />
              </button>
              <span
                id={helpId}
                role="tooltip"
                className="pointer-events-none invisible absolute left-0 top-6 z-20 w-64 max-w-[calc(100vw-2rem)] rounded-md border border-border bg-panel px-2.5 py-2 text-xs font-normal normal-case leading-5 text-text opacity-0 shadow-lg transition-opacity peer-hover:visible peer-hover:opacity-100 peer-focus:visible peer-focus:opacity-100"
              >
                {help}
              </span>
            </>
          ) : null}
        </div>
        {description ? <p className="mt-1 text-xs text-muted">{description}</p> : null}
      </div>
      {actions ? <div className="shrink-0">{actions}</div> : null}
    </div>
  );
}
