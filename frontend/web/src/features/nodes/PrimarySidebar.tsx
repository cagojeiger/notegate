import { useState } from "react";

import type { NodeSummary, Space } from "../../api/types";
import { Tabs } from "../../shared/ui";
import { BrowseActions } from "./BrowseActions";
import { NodeContextMenu } from "./NodeContextMenu";
import { PrimarySidebarSections } from "./PrimarySidebarSections";
import type { NodeContextHandler } from "./types";
import { useSidebarKeyboardNavigation } from "./useSidebarKeyboardNavigation";

const browseTabs: Array<{ id: "browse"; label: string; controls: string }> = [
  { id: "browse", label: "Browse", controls: "browse-sidebar-panel" }
];

export function PrimarySidebar({
  activeSpace,
  openedNodeId,
  inspectedNodeId,
  expandedFolderIds,
  onToggleFolder,
  onInspectNode,
  onOpenNode,
  onOpenNodeInNewGroup,
  onCreateFolder,
  onCreateText,
  onRecordAudio,
  onFileSelected,
  onRenameSpace,
  onDeleteSpace,
  onRenameNode,
  onMoveNode,
  onMoveNodeToFolder,
  onDeleteNode,
  onDownloadFile,
  onCollapseTree,
  onCreateInFolder,
  onUploadInFolder,
  canWriteActiveSpace,
  canManageActiveSpace,
  canOpenInNewGroup
}: {
  activeSpace: Space | null;
  openedNodeId: string | null;
  inspectedNodeId: string | null;
  expandedFolderIds: Set<string>;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  canOpenInNewGroup: boolean;
  onToggleFolder: (nodeId: string) => void;
  onInspectNode: (node: NodeSummary) => void;
  onOpenNode: (node: NodeSummary) => void;
  onOpenNodeInNewGroup: (node: NodeSummary) => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onRecordAudio: () => void;
  onFileSelected: (file: File | null) => void;
  onRenameSpace: () => void;
  onDeleteSpace: () => void;
  onRenameNode: (node: NodeSummary) => void;
  onMoveNode: (node: NodeSummary) => void;
  onMoveNodeToFolder: (node: NodeSummary, folder: NodeSummary) => void;
  onDeleteNode: (node: NodeSummary) => void;
  onDownloadFile: (node: NodeSummary) => void;
  onCollapseTree: () => void;
  onCreateInFolder: (folder: NodeSummary, kind: "folder" | "text") => void;
  onUploadInFolder: (folder: NodeSummary, file: File | null) => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number; node: NodeSummary } | null>(null);
  const { asideRef, onSidebarKeyDown, registerTreeNavigation } = useSidebarKeyboardNavigation();

  const onNodeContextMenu: NodeContextHandler = (node, event) => {
    event.preventDefault();
    onInspectNode(node);
    setMenu({ x: event.clientX, y: event.clientY, node });
  };

  return (
    <aside ref={asideRef} onKeyDown={onSidebarKeyDown} className="flex h-full w-full min-h-0 flex-col border-r border-seam bg-panel font-ui">
      <div className="flex h-workbench-header shrink-0 items-end border-b border-seam px-2">
        <Tabs items={browseTabs} value="browse" onChange={() => {}} label="Primary sidebar sections" variant="header" />
      </div>
      {activeSpace ? (
        <PrimarySidebarSections
          activeSpace={activeSpace}
          openedNodeId={openedNodeId}
          inspectedNodeId={inspectedNodeId}
          expandedFolderIds={expandedFolderIds}
          onToggleFolder={onToggleFolder}
          onInspectNode={onInspectNode}
          onOpenNode={onOpenNode}
          onNodeContextMenu={onNodeContextMenu}
          onMoveNodeToFolder={onMoveNodeToFolder}
          treeHeaderActions={(
            <BrowseActions
              activeSpace={activeSpace}
              canWriteActiveSpace={canWriteActiveSpace}
              canManageActiveSpace={canManageActiveSpace}
              onCreateFolder={onCreateFolder}
              onCreateText={onCreateText}
              onRecordAudio={onRecordAudio}
              onFileSelected={onFileSelected}
              onCollapseTree={onCollapseTree}
              onRenameSpace={onRenameSpace}
              onDeleteSpace={onDeleteSpace}
            />
          )}
          onTreeNavigationChange={registerTreeNavigation}
          canWriteActiveSpace={canWriteActiveSpace}
        />
      ) : (
        <div id="browse-sidebar-panel" role="tabpanel" aria-labelledby="browse-sidebar-panel-tab" className="p-4 text-sm text-muted">Create a space to start.</div>
      )}
      {menu ? (
        <NodeContextMenu
          menu={menu}
          qualifiedPath={activeSpace?.id === menu.node.space_id ? `${activeSpace.name}:${menu.node.path}` : null}
          onClose={() => setMenu(null)}
          onOpenNode={onOpenNode}
          onOpenInNewGroup={onOpenNodeInNewGroup}
          canOpenInNewGroup={canOpenInNewGroup}
          onRenameNode={onRenameNode}
          onMoveNode={onMoveNode}
          onDeleteNode={onDeleteNode}
          onDownloadFile={onDownloadFile}
          onCreateInFolder={onCreateInFolder}
          onUploadInFolder={onUploadInFolder}
          canWriteActiveSpace={canWriteActiveSpace}
        />
      ) : null}
    </aside>
  );
}
