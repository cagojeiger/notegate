import type { DragEvent } from "react";
import { useRef } from "react";
import { ChevronRight } from "lucide-react";

import type { RestNode } from "../../api/types";
import { nodeIcon } from "./nodeDisplay";
import type { NodeContextHandler } from "./types";

export function NodeRow({
  node,
  depth,
  selected,
  expanded,
  meta,
  suffix,
  dropTarget,
  onToggleFolder,
  onOpenNode,
  onNodeContextMenu,
  onDragStartNode,
  onDragOverNode,
  onDropOnNode,
  onDragEndNode
}: {
  node: RestNode;
  depth: number;
  selected: boolean;
  expanded?: boolean;
  meta?: string;
  suffix?: string;
  dropTarget?: boolean;
  onToggleFolder?: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onNodeContextMenu: NodeContextHandler;
  onDragStartNode?: (node: RestNode) => void;
  onDragOverNode?: (node: RestNode, event: DragEvent<HTMLDivElement>) => void;
  onDropOnNode?: (node: RestNode, event: DragEvent<HTMLDivElement>) => void;
  onDragEndNode?: () => void;
}) {
  const Icon = nodeIcon(node);
  const draggable = node.parent_id !== null && Boolean(onDragStartNode);
  const longPressRef = useRef<number | null>(null);
  function clearLongPress() {
    if (longPressRef.current === null) return;
    window.clearTimeout(longPressRef.current);
    longPressRef.current = null;
  }
  function handleOpen() {
    if (node.kind === "folder") onToggleFolder?.(node.id);
    onOpenNode(node);
  }
  return (
    <div
      data-node-row
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
      className={`group flex w-full items-center gap-1 rounded-[9px] py-1.5 pr-2 text-sm transition ${selected ? "bg-[var(--ng-selection)] text-text" : "text-muted hover:bg-[var(--ng-hover)] hover:text-text"} ${dropTarget ? "ring-1 ring-inset ring-primary bg-[var(--ng-selection)] text-text" : ""} ${draggable ? "cursor-grab active:cursor-grabbing" : ""}`}
      style={{ paddingLeft: `${8 + depth * 14}px` }}
      onContextMenu={(event) => { event.stopPropagation(); onNodeContextMenu(node, event); }}
    >
      {node.kind === "folder" ? <button className="grid size-4 place-items-center" onClick={() => onToggleFolder?.(node.id)}><ChevronRight size={13} className={expanded ? "rotate-90 transition" : "transition"} /></button> : <span className="size-4" />}
      <button data-node-open className="flex min-w-0 flex-1 items-center gap-2 text-left outline-none focus-visible:rounded focus-visible:ring-2 focus-visible:ring-primary/50" onClick={handleOpen}>
        <Icon size={15} className="shrink-0" />
        <span className="min-w-0 flex-1">
          <span className="block truncate">{node.name}</span>
          {meta ? <span className="block truncate text-xs text-faint">{meta}</span> : null}
        </span>
      </button>
      {suffix ? <span className="shrink-0 text-[10px] tabular-nums text-faint">{suffix}</span> : null}
    </div>
  );
}
