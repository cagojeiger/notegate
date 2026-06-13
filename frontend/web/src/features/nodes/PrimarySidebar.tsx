import { useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";

import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { NodeContextMenu } from "./NodeContextMenu";
import { RecentSection } from "./RecentSection";
import { SpaceHeader } from "./SpaceHeader";
import { TreeSection } from "./TreeSection";
import type { NodeContextHandler } from "./types";

export function PrimarySidebar({
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  onToggleFolder,
  onOpenNode,
  onCreateFolder,
  onCreateText,
  onFileSelected,
  onRenameSpace,
  onDeleteSpace,
  onRenameNode,
  onDeleteNode,
  onCollapseTree,
  onCreateInFolder,
  onUploadInFolder
}: {
  activeSpace: Space | null;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onFileSelected: (file: File | null) => void;
  onRenameSpace: () => void;
  onDeleteSpace: () => void;
  onRenameNode: (node: RestNode) => void;
  onDeleteNode: (node: RestNode) => void;
  onCollapseTree: () => void;
  onCreateInFolder: (folder: RestNode, kind: "folder" | "text") => void;
  onUploadInFolder: (folder: RestNode, file: File | null) => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number; node: RestNode } | null>(null);
  const treeRatio = useUiStore((state) => state.treeRatio);
  const setTreeRatio = useUiStore((state) => state.setTreeRatio);
  const treeSectionOpen = useUiStore((state) => state.treeSectionOpen);
  const recentSectionOpen = useUiStore((state) => state.recentSectionOpen);
  const recentDensity = useUiStore((state) => state.recentDensity);
  const toggleTreeSection = useUiStore((state) => state.toggleTreeSection);
  const toggleRecentSection = useUiStore((state) => state.toggleRecentSection);
  const toggleRecentDensity = useUiStore((state) => state.toggleRecentDensity);
  const gridRef = useRef<HTMLDivElement>(null);
  const asideRef = useRef<HTMLElement>(null);

  function onSidebarKeyDown(event: ReactKeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const buttons = Array.from(asideRef.current?.querySelectorAll<HTMLButtonElement>("[data-node-open]") ?? []);
    if (buttons.length === 0) return;
    event.preventDefault();
    const current = document.activeElement as HTMLElement | null;
    const index = current ? buttons.indexOf(current as HTMLButtonElement) : -1;
    const next = event.key === "ArrowDown" ? Math.min(index + 1, buttons.length - 1) : Math.max(index <= 0 ? 0 : index - 1, 0);
    buttons[next]?.focus();
  }

  const onNodeContextMenu: NodeContextHandler = (node, event) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, node });
  };

  const bothSectionsOpen = treeSectionOpen && recentSectionOpen;
  function startTreeResize(event: ReactPointerEvent) {
    if (!bothSectionsOpen) return;
    event.preventDefault();
    const rect = gridRef.current?.getBoundingClientRect();
    if (!rect) return;
    const move = (e: PointerEvent) => setTreeRatio((e.clientY - rect.top) / rect.height);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("select-none");
    };
    document.body.classList.add("select-none");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  const gridRows = bothSectionsOpen
    ? `${treeRatio}fr 6px ${1 - treeRatio}fr`
    : treeSectionOpen
      ? "1fr 6px auto"
      : recentSectionOpen
        ? "auto 6px 1fr"
        : "auto 6px auto";

  return (
    <aside ref={asideRef} onKeyDown={onSidebarKeyDown} className="flex h-full w-full min-h-0 flex-col border-r border-seam bg-panel">
      <SpaceHeader activeSpace={activeSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} onRenameSpace={onRenameSpace} onDeleteSpace={onDeleteSpace} />
      {activeSpace ? (
        <div ref={gridRef} className="grid min-h-0 min-w-0 flex-1 content-start" style={{ gridTemplateRows: gridRows }}>
          <TreeSection
            activeSpace={activeSpace}
            activeNodeId={activeNodeId}
            expandedFolderIds={expandedFolderIds}
            open={treeSectionOpen}
            onToggle={toggleTreeSection}
            onCollapseTree={onCollapseTree}
            onToggleFolder={onToggleFolder}
            onOpenNode={onOpenNode}
            onNodeContextMenu={onNodeContextMenu}
          />
          <div onPointerDown={startTreeResize} className={`border-y border-seam bg-surface ${bothSectionsOpen ? "cursor-row-resize transition-colors hover:bg-primary/30" : ""}`} aria-hidden="true" />
          <RecentSection
            activeSpace={activeSpace}
            activeNodeId={activeNodeId}
            density={recentDensity}
            open={recentSectionOpen}
            onToggle={toggleRecentSection}
            onToggleDensity={toggleRecentDensity}
            onOpenNode={onOpenNode}
            onNodeContextMenu={onNodeContextMenu}
          />
        </div>
      ) : (
        <div className="p-4 text-sm text-muted">Create a space to start.</div>
      )}
      {menu ? (
        <NodeContextMenu
          menu={menu}
          onClose={() => setMenu(null)}
          onOpenNode={onOpenNode}
          onRenameNode={onRenameNode}
          onDeleteNode={onDeleteNode}
          onCreateInFolder={onCreateInFolder}
          onUploadInFolder={onUploadInFolder}
        />
      ) : null}
    </aside>
  );
}
