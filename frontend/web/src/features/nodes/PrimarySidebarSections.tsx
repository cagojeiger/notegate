import type { NodeSummary, Space } from "../../api/types";
import { RecentSection } from "./RecentSection";
import { TreeSection } from "./TreeSection";
import type { NodeContextHandler, TreeKeyboardNavigationRegistrar } from "./types";
import { usePrimarySidebarSections } from "./usePrimarySidebarSections";

export function PrimarySidebarSections({
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  onToggleFolder,
  onOpenNode,
  onNodeContextMenu,
  onMoveNodeToFolder,
  onCollapseTree,
  onTreeNavigationChange,
  canWriteActiveSpace
}: {
  activeSpace: Space;
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  canWriteActiveSpace: boolean;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: NodeSummary) => void;
  onNodeContextMenu: NodeContextHandler;
  onMoveNodeToFolder: (node: NodeSummary, folder: NodeSummary) => void;
  onCollapseTree: () => void;
  onTreeNavigationChange: TreeKeyboardNavigationRegistrar;
}) {
  const sections = usePrimarySidebarSections();
  return (
    <div ref={sections.gridRef} className="grid min-h-0 min-w-0 flex-1 content-start" style={{ gridTemplateRows: sections.gridRows }}>
      <TreeSection
        activeSpace={activeSpace}
        activeNodeId={activeNodeId}
        expandedFolderIds={expandedFolderIds}
        open={sections.treeSectionOpen}
        onToggle={sections.toggleTreeSection}
        onCollapseTree={onCollapseTree}
        onTreeNavigationChange={onTreeNavigationChange}
        onToggleFolder={onToggleFolder}
        onOpenNode={onOpenNode}
        onNodeContextMenu={onNodeContextMenu}
        onMoveNodeToFolder={onMoveNodeToFolder}
        canWriteActiveSpace={canWriteActiveSpace}
      />
      <div onPointerDown={sections.startTreeResize} className={`border-y border-seam bg-surface ${sections.bothSectionsOpen ? "cursor-row-resize transition-colors hover:bg-[var(--ng-selection)]" : ""}`} aria-hidden="true" />
      <RecentSection
        activeSpace={activeSpace}
        activeNodeId={activeNodeId}
        density={sections.recentDensity}
        open={sections.recentSectionOpen}
        onToggle={sections.toggleRecentSection}
        onToggleDensity={sections.toggleRecentDensity}
        onOpenNode={onOpenNode}
        onNodeContextMenu={onNodeContextMenu}
      />
    </div>
  );
}
