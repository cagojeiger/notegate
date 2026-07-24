import { useState, type MouseEvent } from "react";
import { nodeIcon } from "../nodes/nodeDisplay";

import type { RestNode, Space } from "../../api/types";
import { MAX_EDITOR_GROUPS, type EditorGroup, type EditorPresentation, type OpenedNodeRef } from "../../shared/model/workbench";
import { EditorGroupHeader } from "./EditorGroupHeader";
import { EmptyEditor } from "./EmptyEditor";
import { FileDetailView } from "./FileDetailView";
import { FolderDetailView } from "./FolderDetailView";
import { NodeActionMenu } from "./NodeActionMenu";
import { NodeContextMenu } from "../nodes/NodeContextMenu";
import { OpenedNodeGuard } from "./OpenedNodeGuard";
import { TextEditorView } from "./TextEditorView";
import type { EditorNavigationActions, NodeActions } from "./types";

type EditorAreaProps = NodeActions & {
  groups: EditorGroup[];
  activeGroupIndex: number;
  presentation?: EditorPresentation;
  visibleGroupCount?: number;
  activeSpace: Space | null;
  onFocusGroup: (index: number) => void;
  onOpenNode: (node: RestNode) => void;
  onCloseGroup: (index: number) => void;
  onSetGroupMode: (index: number, mode: "preview" | "edit") => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onFileSelected: (file: File | null) => void;
  onDownloadFile: (node: RestNode) => void;
  canWriteActiveSpace: boolean;
} & EditorNavigationActions;

export function EditorArea({ groups, activeGroupIndex, presentation = "split", visibleGroupCount = groups.length, activeSpace, canWriteActiveSpace, onFocusGroup, onOpenNode, onOpenNodeInNewGroup, onOpenMarkdownLink, onCloseGroup, onSetGroupMode, onCreateFolder, onCreateText, onFileSelected, onDownloadFile, onRenameNode, onMoveNode, onDeleteNode }: EditorAreaProps) {
  const multiple = groups.length > 1;
  const [headerMenu, setHeaderMenu] = useState<{ x: number; y: number; node: RestNode; groupIndex: number } | null>(null);
  const visibleRange = editorGroupVisibleRange(groups.length, activeGroupIndex, visibleGroupCount);
  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden">
      {groups.map((group, index) => {
        const active = index === activeGroupIndex;
        const outsideVisibleRange = index < visibleRange.start || index >= visibleRange.end;
        const hidden = presentation === "focused" ? !active : outsideVisibleRange;
        return (
          <section
            key={group.id}
            data-editor-group
            data-active={active ? "true" : "false"}
            onMouseDown={() => onFocusGroup(index)}
            className={`${hidden ? "hidden" : "flex"} min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-[var(--ng-editor)] ${index > 0 ? "border-l border-seam" : ""} ${active || presentation === "focused" ? "" : "max-md:hidden"} ${multiple ? (active ? "relative z-10 outline outline-1 -outline-offset-1 outline-[var(--ng-active-border)]" : "outline outline-1 -outline-offset-1 outline-transparent") : ""}`}
          >
            <GroupBody
              active={active}
              groupId={group.id}
              nodeRef={group.nodeRef}
              mode={group.mode}
              activeSpace={activeSpace}
              canWriteActiveSpace={canWriteActiveSpace}
              canClose={multiple}
              canOpenInNewGroup={groups.length < MAX_EDITOR_GROUPS}
              onClose={() => onCloseGroup(index)}
              onSetMode={(mode) => onSetGroupMode(index, mode)}
              onOpenNodeInNewGroup={onOpenNodeInNewGroup}
              onOpenMarkdownLink={onOpenMarkdownLink}
              onCreateFolder={onCreateFolder}
              onCreateText={onCreateText}
              onFileSelected={onFileSelected}
              onRenameNode={onRenameNode}
              onMoveNode={onMoveNode}
              onDeleteNode={onDeleteNode}
              onHeaderContextMenu={(node, event) => {
                event.preventDefault();
                onFocusGroup(index);
                setHeaderMenu({ x: event.clientX, y: event.clientY, node, groupIndex: index });
              }}
            />
          </section>
        );
      })}
      {headerMenu ? (
        <NodeContextMenu
          menu={headerMenu}
          canWriteActiveSpace={canWriteActiveSpace}
          canOpenInNewGroup={groups.length < MAX_EDITOR_GROUPS}
          showCreateActions={false}
          onClose={() => setHeaderMenu(null)}
          onOpenNode={onOpenNode}
          onOpenInNewGroup={onOpenNodeInNewGroup}
          onCloseGroup={groups.length > 1 ? () => onCloseGroup(headerMenu.groupIndex) : undefined}
          onRenameNode={onRenameNode}
          onMoveNode={onMoveNode}
          onDeleteNode={onDeleteNode}
          onDownloadFile={onDownloadFile}
          onCreateInFolder={() => undefined}
          onUploadInFolder={() => undefined}
        />
      ) : null}
    </div>
  );
}

function editorGroupVisibleRange(totalGroups: number, activeIndex: number, visibleGroupCount: number): { start: number; end: number } {
  const total = Math.max(1, totalGroups);
  const count = Math.max(1, Math.min(total, visibleGroupCount));
  const start = Math.min(Math.max(0, activeIndex), total - count);
  return { start, end: start + count };
}

function GroupBody({ active, groupId, nodeRef, mode, activeSpace, canWriteActiveSpace, canClose, canOpenInNewGroup, onClose, onSetMode, onOpenNodeInNewGroup, onOpenMarkdownLink, onCreateFolder, onCreateText, onFileSelected, onRenameNode, onMoveNode, onDeleteNode, onHeaderContextMenu }: NodeActions & EditorNavigationActions & { active: boolean; groupId: number; nodeRef: OpenedNodeRef | null; mode: "preview" | "edit"; activeSpace: Space | null; canWriteActiveSpace: boolean; canClose: boolean; canOpenInNewGroup: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void; onHeaderContextMenu: (node: RestNode, event: MouseEvent) => void }) {
  if (!nodeRef) {
    return (
      <>
        <EditorGroupHeader title="Open a node" canClose={canClose} onClose={onClose} active={active} />
        <EmptyEditor activeSpace={activeSpace} canWriteActiveSpace={canWriteActiveSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} />
      </>
    );
  }
  return (
    <OpenedNodeGuard nodeRef={nodeRef}>
      {(freshNode) => (
        <NodeGroupContent
          active={active}
          groupId={groupId}
          node={freshNode}
          mode={mode}
          canWriteActiveSpace={canWriteActiveSpace}
          canClose={canClose}
          canOpenInNewGroup={canOpenInNewGroup}
          onClose={onClose}
          onSetMode={onSetMode}
          onOpenNodeInNewGroup={onOpenNodeInNewGroup}
          onOpenMarkdownLink={onOpenMarkdownLink}
          onRenameNode={onRenameNode}
          onMoveNode={onMoveNode}
          onDeleteNode={onDeleteNode}
          onHeaderContextMenu={onHeaderContextMenu}
        />
      )}
    </OpenedNodeGuard>
  );
}

function NodeGroupContent({ active, groupId, node, mode, canWriteActiveSpace, canClose, canOpenInNewGroup, onClose, onSetMode, onOpenNodeInNewGroup, onOpenMarkdownLink, onRenameNode, onMoveNode, onDeleteNode, onHeaderContextMenu }: NodeActions & EditorNavigationActions & { active: boolean; groupId: number; node: RestNode; mode: "preview" | "edit"; canWriteActiveSpace: boolean; canClose: boolean; canOpenInNewGroup: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void; onHeaderContextMenu: (node: RestNode, event: MouseEvent) => void }) {
  if (node.kind === "text") {
    return <TextEditorView active={active} groupId={groupId} node={node} latestNode={node} mode={mode} canWriteActiveSpace={canWriteActiveSpace} canOpenInNewGroup={canOpenInNewGroup} canClose={canClose} onClose={onClose} onSetMode={onSetMode} onOpenNodeInNewGroup={onOpenNodeInNewGroup} onOpenMarkdownLink={onOpenMarkdownLink} onRenameNode={onRenameNode} onMoveNode={onMoveNode} onDeleteNode={onDeleteNode} />;
  }
  const Icon = nodeIcon(node);
  return (
    <>
      <EditorGroupHeader
        active={active}
        title={node.name}
        icon={<Icon size={17} />}
        canClose={canClose}
        onClose={onClose}
        onContextMenu={(event) => onHeaderContextMenu(node, event)}
        actions={<NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null || !canWriteActiveSpace} />}
      />
      {node.kind === "file" ? <FileDetailView node={node} /> : <FolderDetailView node={node} />}
    </>
  );
}
