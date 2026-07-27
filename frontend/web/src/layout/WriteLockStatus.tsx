import { LockKeyhole, LockKeyholeOpen } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";

import type { WriteLockSource } from "../api/types";
import { Card } from "../shared/ui";

const PANEL_GUTTER = 12;
const PANEL_WIDTH = 272;
const PANEL_MAX_HEIGHT = 240;

type Position = {
  left: number;
  top: number;
};

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
  const panelRef = useRef<HTMLDivElement>(null);
  const panelId = useId();
  const [position, setPosition] = useState<Position | null>(null);
  const locked = directlyLocked || inheritedSources.length > 0;

  function close({ restoreFocus = false } = {}) {
    setPosition(null);
    if (restoreFocus) triggerRef.current?.focus();
  }

  function open() {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return;

    const width = Math.min(PANEL_WIDTH, window.innerWidth - PANEL_GUTTER * 2);
    const estimatedHeight = Math.min(
      PANEL_MAX_HEIGHT,
      48 + inheritedSources.length * 36
    );
    const left = Math.min(
      Math.max(PANEL_GUTTER, rect.right - width),
      window.innerWidth - width - PANEL_GUTTER
    );
    const below = rect.bottom + 6;
    const top = below + estimatedHeight <= window.innerHeight - PANEL_GUTTER
      ? below
      : Math.max(PANEL_GUTTER, rect.top - estimatedHeight - 6);

    setPosition({ left, top });
  }

  useEffect(() => {
    if (!position) return;
    panelRef.current?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setPosition(null);
      triggerRef.current?.focus();
    }

    function onViewportChange(event: Event) {
      if (
        event.type === "scroll"
        && event.target instanceof Node
        && panelRef.current?.contains(event.target)
      ) {
        return;
      }
      setPosition(null);
    }

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("scroll", onViewportChange, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("scroll", onViewportChange, true);
    };
  }, [position]);

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
          className="shrink-0 rounded px-1.5 py-1 text-xs font-medium text-primary outline-none hover:bg-panel-strong focus-visible:ring-2 focus-visible:ring-primary/45"
          aria-expanded={position !== null}
          aria-controls={position ? panelId : undefined}
          aria-haspopup="dialog"
          onClick={() => {
            if (position) {
              close();
            } else {
              open();
            }
          }}
        >
          {sourceLabel}
        </button>
      ) : null}
      {position
        ? createPortal(
            <>
              <button
                type="button"
                className="fixed inset-0 z-40 cursor-default bg-transparent"
                aria-label="Close lock sources"
                onClick={() => close({ restoreFocus: true })}
              />
              <div
                ref={panelRef}
                id={panelId}
                role="dialog"
                aria-label="Inherited lock sources"
                tabIndex={-1}
                className="fixed z-50 w-[272px] max-w-[calc(100vw-1.5rem)] outline-none"
                style={{ left: position.left, top: position.top }}
              >
                <Card
                  padding="none"
                  className="overflow-hidden shadow-[var(--ng-focus-shadow)]"
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
              </div>
            </>,
            document.body
          )
        : null}
    </div>
  );
}
