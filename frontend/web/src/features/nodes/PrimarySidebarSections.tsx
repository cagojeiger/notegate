import type { ReactNode } from "react";

import type { NodeSummary, Space } from "../../api/types";
import { WORKBENCH_LAYOUT } from "../../shared/model/workbenchLayout";
import { ResizeSeparator } from "../../shared/ui";
import { RecentSection } from "./RecentSection";
import { TreeSection } from "./TreeSection";
import type { NodeContextHandler, TreeKeyboardNavigationRegistrar } from "./types";
import { usePrimarySidebarSections } from "./usePrimarySidebarSections";

export function PrimarySidebarSections({
  activeSpace,
  openedNodeId,
  inspectedNodeId,
  expandedFolderIds,
  onToggleFolder,
  onInspectNode,
  onOpenNode,
  onNodeContextMenu,
  onMoveNodeToFolder,
  treeHeaderActions,
  onTreeNavigationChange,
  canWriteActiveSpace
}: {
  activeSpace: Space;
  openedNodeId: string | null;
  inspectedNodeId: string | null;
  expandedFolderIds: Set<string>;
  canWriteActiveSpace: boolean;
  onToggleFolder: (nodeId: string) => void;
  onInspectNode: (node: NodeSummary) => void;
  onOpenNode: (node: NodeSummary) => void;
  onNodeContextMenu: NodeContextHandler;
  onMoveNodeToFolder: (node: NodeSummary, folder: NodeSummary) => void;
  treeHeaderActions: ReactNode;
  onTreeNavigationChange: TreeKeyboardNavigationRegistrar;
}) {
  const sections = usePrimarySidebarSections();
  return (
    <div id="browse-sidebar-panel" role="tabpanel" aria-labelledby="browse-sidebar-panel-tab" ref={sections.gridRef} className="grid min-h-0 min-w-0 flex-1 content-start" style={{ gridTemplateRows: sections.gridRows }}>
      <TreeSection
        activeSpace={activeSpace}
        openedNodeId={openedNodeId}
        inspectedNodeId={inspectedNodeId}
        expandedFolderIds={expandedFolderIds}
        open={sections.treeSectionOpen}
        onToggle={sections.toggleTreeSection}
        headerActions={treeHeaderActions}
        onTreeNavigationChange={onTreeNavigationChange}
        onToggleFolder={onToggleFolder}
        onInspectNode={onInspectNode}
        onOpenNode={onOpenNode}
        onNodeContextMenu={onNodeContextMenu}
        onMoveNodeToFolder={onMoveNodeToFolder}
        canWriteActiveSpace={canWriteActiveSpace}
      />
      <div
        className="relative"
      >
        {sections.bothSectionsOpen ? (
          <ResizeSeparator
            orientation="horizontal"
            label="Resize Files section"
            value={Math.round(sections.treeRatio * 100)}
            min={WORKBENCH_LAYOUT.minTreeRatio * 100}
            max={WORKBENCH_LAYOUT.maxTreeRatio * 100}
            step={5}
            valueText={`${Math.round(sections.treeRatio * 100)}% Files`}
            controls="files-section"
            onPointerDown={sections.startTreeResize}
            onValueChange={(value) => sections.setTreeRatio(value / 100)}
          />
        ) : (
          <span className="absolute inset-x-0 top-1/2 h-px bg-seam" aria-hidden="true" />
        )}
      </div>
      <RecentSection
        activeSpace={activeSpace}
        openedNodeId={openedNodeId}
        inspectedNodeId={inspectedNodeId}
        density={sections.recentDensity}
        open={sections.recentSectionOpen}
        onToggle={sections.toggleRecentSection}
        onToggleDensity={sections.toggleRecentDensity}
        onOpenNode={onOpenNode}
        onInspectNode={onInspectNode}
        onNodeContextMenu={onNodeContextMenu}
      />
    </div>
  );
}
