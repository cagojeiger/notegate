import { useState } from "react";

import type { RestNode } from "../../entities/node/model";
import type { Space } from "../../entities/space/model";
import { NodeContextMenu } from "./NodeContextMenu";
import { SpaceHeader } from "./SpaceHeader";
import { PrimarySidebarSections } from "./PrimarySidebarSections";
import type { NodeContextHandler } from "./types";
import { useSidebarKeyboardNavigation } from "./useSidebarKeyboardNavigation";

export function PrimarySidebar({
  activeSpace,
  activeNodeId,
  expandedFolderIds,
  onToggleFolder,
  onOpenNode,
  onOpenNodeInNewGroup,
  onCreateFolder,
  onCreateText,
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
  activeNodeId: string | null;
  expandedFolderIds: Set<string>;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  canOpenInNewGroup: boolean;
  onToggleFolder: (nodeId: string) => void;
  onOpenNode: (node: RestNode) => void;
  onOpenNodeInNewGroup: (node: RestNode) => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onFileSelected: (file: File | null) => void;
  onRenameSpace: () => void;
  onDeleteSpace: () => void;
  onRenameNode: (node: RestNode) => void;
  onMoveNode: (node: RestNode) => void;
  onMoveNodeToFolder: (node: RestNode, folder: RestNode) => void;
  onDeleteNode: (node: RestNode) => void;
  onDownloadFile: (node: RestNode) => void;
  onCollapseTree: () => void;
  onCreateInFolder: (folder: RestNode, kind: "folder" | "text") => void;
  onUploadInFolder: (folder: RestNode, file: File | null) => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number; node: RestNode } | null>(null);
  const { asideRef, onSidebarKeyDown, registerTreeNavigation } = useSidebarKeyboardNavigation();

  const onNodeContextMenu: NodeContextHandler = (node, event) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, node });
  };

  return (
    <aside ref={asideRef} onKeyDown={onSidebarKeyDown} className="flex h-full w-full min-h-0 flex-col border-r border-seam bg-panel">
      <SpaceHeader activeSpace={activeSpace} canWriteActiveSpace={canWriteActiveSpace} canManageActiveSpace={canManageActiveSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} onRenameSpace={onRenameSpace} onDeleteSpace={onDeleteSpace} />
      {activeSpace ? (
        <PrimarySidebarSections
          activeSpace={activeSpace}
          activeNodeId={activeNodeId}
          expandedFolderIds={expandedFolderIds}
          onToggleFolder={onToggleFolder}
          onOpenNode={onOpenNode}
          onNodeContextMenu={onNodeContextMenu}
          onMoveNodeToFolder={onMoveNodeToFolder}
          onCollapseTree={onCollapseTree}
          onTreeNavigationChange={registerTreeNavigation}
          canWriteActiveSpace={canWriteActiveSpace}
        />
      ) : (
        <div className="p-4 text-sm text-muted">Create a space to start.</div>
      )}
      {menu ? (
        <NodeContextMenu
          menu={menu}
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
