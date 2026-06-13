import { useState } from "react";

import type { RestNode, Space } from "../../api/types";
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
  const { asideRef, onSidebarKeyDown } = useSidebarKeyboardNavigation();

  const onNodeContextMenu: NodeContextHandler = (node, event) => {
    event.preventDefault();
    setMenu({ x: event.clientX, y: event.clientY, node });
  };

  return (
    <aside ref={asideRef} onKeyDown={onSidebarKeyDown} className="flex h-full w-full min-h-0 flex-col border-r border-seam bg-panel">
      <SpaceHeader activeSpace={activeSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} onRenameSpace={onRenameSpace} onDeleteSpace={onDeleteSpace} />
      {activeSpace ? (
        <PrimarySidebarSections
          activeSpace={activeSpace}
          activeNodeId={activeNodeId}
          expandedFolderIds={expandedFolderIds}
          onToggleFolder={onToggleFolder}
          onOpenNode={onOpenNode}
          onNodeContextMenu={onNodeContextMenu}
          onCollapseTree={onCollapseTree}
        />
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
