import type { DragEvent } from "react";
import { useId, useRef } from "react";
import { ChevronRight, LockKeyhole } from "lucide-react";

import type { NodeSummary } from "../../api/types";
import { nodeIcon } from "./nodeDisplay";
import type { NodeContextHandler } from "./types";

export function NodeRow({
  node,
  depth,
  inspected,
  opened,
  expanded,
  meta,
  suffix,
  dropTarget,
  onToggleFolder,
  onInspectNode,
  onOpenNode,
  onNodeContextMenu,
  onDragStartNode,
  onDragOverNode,
  onDropOnNode,
  onDragEndNode
}: {
  node: NodeSummary;
  depth: number;
  inspected: boolean;
  opened: boolean;
  expanded?: boolean;
  meta?: string;
  suffix?: string;
  dropTarget?: boolean;
  onToggleFolder?: (nodeId: string) => void;
  onInspectNode: (node: NodeSummary) => void;
  onOpenNode: (node: NodeSummary) => void;
  onNodeContextMenu: NodeContextHandler;
  onDragStartNode?: (node: NodeSummary) => void;
  onDragOverNode?: (node: NodeSummary, event: DragEvent<HTMLDivElement>) => void;
  onDropOnNode?: (node: NodeSummary, event: DragEvent<HTMLDivElement>) => void;
  onDragEndNode?: () => void;
}) {
  const Icon = nodeIcon(node);
  const draggable = node.parent_id !== null && Boolean(onDragStartNode);
  const longPressRef = useRef<number | null>(null);
  const lockDescriptionId = useId();
  function clearLongPress() {
    if (longPressRef.current === null) return;
    window.clearTimeout(longPressRef.current);
    longPressRef.current = null;
  }
  function handleToggleFolder() {
    onInspectNode(node);
    onToggleFolder?.(node.id);
  }
  function handleOpen() {
    onInspectNode(node);
    if (node.kind === "folder" && onToggleFolder) {
      onToggleFolder(node.id);
      return;
    }
    onOpenNode(node);
  }
  return (
    <div
      data-node-row
      data-inspected={inspected ? "true" : undefined}
      draggable={draggable}
      onDragStart={(event) => {
        if (!draggable) return;
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData("text/plain", node.id);
        onDragStartNode?.(node);
      }}
      onDragOver={(event) => onDragOverNode?.(node, event)}
      onDrop={(event) => onDropOnNode?.(node, event)}
      onDragEnd={onDragEndNode}
      onTouchStart={(event) => {
        clearLongPress();
        const touch = event.touches[0];
        if (!touch) return;
        longPressRef.current = window.setTimeout(() => {
          onNodeContextMenu(node, { clientX: touch.clientX, clientY: touch.clientY, preventDefault: () => undefined });
          longPressRef.current = null;
        }, 520);
      }}
      onTouchMove={clearLongPress}
      onTouchEnd={clearLongPress}
      onTouchCancel={clearLongPress}
      className={`group relative flex w-full items-center gap-1 rounded-[9px] py-1.5 pr-2 text-sm transition active:bg-[var(--ng-selection)] active:text-text ${inspected ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"} ${dropTarget ? "ring-1 ring-inset ring-primary bg-[var(--ng-selection)] text-text" : ""} ${draggable ? "cursor-grab active:cursor-grabbing" : ""}`}
      style={{ paddingLeft: `${8 + depth * 14}px` }}
      onContextMenu={(event) => { event.stopPropagation(); onNodeContextMenu(node, event); }}
    >
      {opened ? <span data-active-indicator className="absolute bottom-1.5 left-0 top-1.5 w-[3px] rounded-r-full bg-primary" aria-hidden="true" /> : null}
      {node.kind === "folder" ? <button aria-label={`${expanded ? "Collapse" : "Expand"} ${node.name}`} className="grid size-6 shrink-0 place-items-center" onClick={handleToggleFolder}><ChevronRight size={13} className={expanded ? "rotate-90 transition" : "transition"} /></button> : <span className="size-6 shrink-0" />}
      <button
        data-node-open
        aria-current={opened ? "page" : undefined}
        aria-describedby={node.effective_write_locked ? lockDescriptionId : undefined}
        className="flex min-w-0 flex-1 items-center gap-2 text-left outline-none focus-visible:rounded focus-visible:ring-2 focus-visible:ring-primary/50"
        onClick={handleOpen}
      >
        <span className="relative grid size-4 shrink-0 place-items-center">
          <Icon size={15} data-node-kind-icon />
          {node.effective_write_locked ? (
            <span
              data-node-lock-indicator
              title="Write locked"
              aria-hidden="true"
              className="absolute -bottom-1 -right-1 grid size-3 place-items-center text-warning"
            >
              <LockKeyhole size={10} strokeWidth={2.5} />
            </span>
          ) : null}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate">{node.name}</span>
          {meta ? <span className="block truncate text-xs text-faint">{meta}</span> : null}
        </span>
      </button>
      {node.effective_write_locked ? (
        <span id={lockDescriptionId} className="sr-only">Write locked</span>
      ) : null}
      {suffix ? <span className="shrink-0 text-[10px] tabular-nums text-faint">{suffix}</span> : null}
    </div>
  );
}
