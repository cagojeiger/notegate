import { ChevronRight, Database, FileText, Folder } from "lucide-react";

import type { RestNode } from "../../api/types";
import type { NodeContextHandler } from "./types";

export function NodeRow({ node, depth, selected, expanded, meta, suffix, onToggleFolder, onOpenNode, onNodeContextMenu }: { node: RestNode; depth: number; selected: boolean; expanded?: boolean; meta?: string; suffix?: string; onToggleFolder?: (nodeId: string) => void; onOpenNode: (node: RestNode) => void; onNodeContextMenu: NodeContextHandler }) {
  const Icon = node.kind === "folder" ? Folder : node.kind === "file" ? Database : FileText;
  function handleOpen() {
    if (node.kind === "folder") onToggleFolder?.(node.id);
    onOpenNode(node);
  }
  return (
    <div
      data-node-row
      className={`group flex w-full items-center gap-1 rounded-lg py-1.5 pr-2 text-sm transition ${selected ? "bg-panel-strong text-text" : "text-muted hover:bg-surface hover:text-text"}`}
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
