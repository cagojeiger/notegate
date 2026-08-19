import { LockKeyhole, LockKeyholeOpen } from "lucide-react";
import { useCallback, useId, useRef, useState } from "react";

import type { WriteLockSource } from "../api/types";
import { AnchoredOverlay, Card } from "../shared/ui";

const PANEL_WIDTH = 272;
const PANEL_MAX_HEIGHT = 240;

export function WriteLockStatus({
  nodeId,
  directlyLocked,
  sources
}: {
  nodeId: string;
  directlyLocked: boolean;
  sources: WriteLockSource[];
}) {
  const inheritedSources = sources.filter((source) => source.node_id !== nodeId);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const [open, setOpen] = useState(false);
  const closePanel = useCallback(() => setOpen(false), []);
  const locked = directlyLocked || inheritedSources.length > 0;

  const status = directlyLocked
    ? "Locked here"
    : inheritedSources.length > 0
      ? "Inherited"
      : "Changes allowed";
  const sourceLabel = directlyLocked
    ? `${inheritedSources.length} inherited`
    : `${inheritedSources.length} ${inheritedSources.length === 1 ? "source" : "sources"}`;

  return (
    <div className="mt-3 flex min-h-8 items-center gap-2 border-t border-seam pt-3">
      <span className="grid size-6 shrink-0 place-items-center text-muted" aria-hidden="true">
        {locked ? <LockKeyhole size={16} /> : <LockKeyholeOpen size={16} />}
      </span>
      <span className="min-w-0 flex-1 truncate text-sm font-medium text-text">{status}</span>
      {inheritedSources.length > 0 ? (
        <button
          ref={triggerRef}
          type="button"
          className="min-h-workbench-control shrink-0 rounded px-1.5 py-1 text-workbench font-medium text-primary outline-none hover:bg-panel-strong focus-visible:ring-2 focus-visible:ring-primary/45 md:min-h-6"
          aria-expanded={open}
          aria-controls={open ? panelId : undefined}
          aria-haspopup="dialog"
          onClick={() => setOpen((value) => !value)}
        >
          {sourceLabel}
        </button>
      ) : null}
      <AnchoredOverlay
        anchorRef={triggerRef}
        open={open}
        onClose={closePanel}
        id={panelId}
        label="Inherited lock sources"
        role="dialog"
        width={PANEL_WIDTH}
        estimatedHeight={Math.min(PANEL_MAX_HEIGHT, 48 + inheritedSources.length * 36)}
      >
        <Card
          padding="none"
          className="w-full overflow-hidden shadow-[var(--ng-focus-shadow)]"
        >
          <div className="border-b border-seam px-3 py-2 text-xs font-semibold uppercase tracking-wide text-muted">
            Inherited lock sources
          </div>
          <ul className="max-h-48 overflow-y-auto p-1.5">
            {inheritedSources.map((source) => (
              <li
                key={source.node_id}
                className="rounded-lg px-2 py-1.5 text-xs text-text"
                title={source.path}
              >
                <span className="block break-words">{source.path}</span>
              </li>
            ))}
          </ul>
        </Card>
      </AnchoredOverlay>
    </div>
  );
}
