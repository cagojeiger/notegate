import { Download } from "lucide-react";
import { useState, type MouseEvent, type ReactNode } from "react";

import type { NodeSummary, RestNode, Space } from "../../api/types";
import { MAX_EDITOR_GROUPS, type EditorPresentation } from "../../shared/model/workbenchLayout";
import { IconButton } from "../../shared/ui";
import type { EditorGroup } from "../../stores/uiStore";
import type { EditorNavigationDirection } from "../../stores/uiStoreReducers";
import { nodeIcon } from "../nodes/nodeDisplay";
import { canMutateNode } from "../nodes/nodeWriteAccess";
import { NodeContextMenu } from "../nodes/NodeContextMenu";
import { EditorGroupHeader } from "./EditorGroupHeader";
import { EditorNavigationControls } from "./EditorNavigationControls";
import { EmptyEditor } from "./EmptyEditor";
import { FileDetailView } from "./FileDetailView";
import { FolderDetailView } from "./FolderDetailView";
import { NodeActionMenu } from "./NodeActionMenu";
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
  onNavigateEditorGroup: (groupId: number, direction: EditorNavigationDirection) => void;
  navigatingGroupIds: ReadonlySet<number>;
  onOpenNode: (node: NodeSummary) => void;
  onCloseGroup: (index: number) => void;
  onSetGroupMode: (index: number, mode: "preview" | "edit") => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onRecordAudio: () => void;
  onFileSelected: (file: File | null) => void;
  onDownloadFile: (node: NodeSummary) => void;
  canWriteActiveSpace: boolean;
} & EditorNavigationActions;

export function EditorArea({ groups, activeGroupIndex, presentation = "split", visibleGroupCount = groups.length, activeSpace, canWriteActiveSpace, onFocusGroup, onNavigateEditorGroup, navigatingGroupIds, onOpenNode, onOpenNodeInNewGroup, onOpenMarkdownLink, onCloseGroup, onSetGroupMode, onCreateFolder, onCreateText, onRecordAudio, onFileSelected, onDownloadFile, onRenameNode, onMoveNode, onDeleteNode }: EditorAreaProps) {
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
              navigationActions={<EditorNavigationControls group={group} pending={navigatingGroupIds.has(group.id)} onNavigate={onNavigateEditorGroup} />}
              node={group.node}
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
              onRecordAudio={onRecordAudio}
              onFileSelected={onFileSelected}
              onDownloadFile={onDownloadFile}
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
          qualifiedPath={activeSpace?.id === headerMenu.node.space_id ? `${activeSpace.name}:${headerMenu.node.path}` : null}
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

function GroupBody({ active, groupId, navigationActions, node, mode, activeSpace, canWriteActiveSpace, canClose, canOpenInNewGroup, onClose, onSetMode, onOpenNodeInNewGroup, onOpenMarkdownLink, onCreateFolder, onCreateText, onRecordAudio, onFileSelected, onDownloadFile, onRenameNode, onMoveNode, onDeleteNode, onHeaderContextMenu }: NodeActions & EditorNavigationActions & { active: boolean; groupId: number; navigationActions: ReactNode; node: RestNode | null; mode: "preview" | "edit"; activeSpace: Space | null; canWriteActiveSpace: boolean; canClose: boolean; canOpenInNewGroup: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void; onCreateFolder: () => void; onCreateText: () => void; onRecordAudio: () => void; onFileSelected: (file: File | null) => void; onDownloadFile: (node: NodeSummary) => void; onHeaderContextMenu: (node: RestNode, event: MouseEvent) => void }) {
  if (!node) {
    return (
      <>
        <EditorGroupHeader title="Choose from Files" navigationActions={navigationActions} canClose={canClose} onClose={onClose} active={active} />
        <EmptyEditor activeSpace={activeSpace} canWriteActiveSpace={canWriteActiveSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onRecordAudio={onRecordAudio} onFileSelected={onFileSelected} />
      </>
    );
  }
  return (
    <OpenedNodeGuard node={node}>
      {(freshNode) => (
        <NodeGroupContent
          active={active}
          groupId={groupId}
          navigationActions={navigationActions}
          node={freshNode}
          qualifiedPath={activeSpace?.id === freshNode.space_id ? `${activeSpace.name}:${freshNode.path}` : null}
          mode={mode}
          canWriteActiveSpace={canWriteActiveSpace}
          canClose={canClose}
          canOpenInNewGroup={canOpenInNewGroup}
          onClose={onClose}
          onSetMode={onSetMode}
          onOpenNodeInNewGroup={onOpenNodeInNewGroup}
          onOpenMarkdownLink={onOpenMarkdownLink}
          onDownloadFile={onDownloadFile}
          onRenameNode={onRenameNode}
          onMoveNode={onMoveNode}
          onDeleteNode={onDeleteNode}
          onHeaderContextMenu={onHeaderContextMenu}
        />
      )}
    </OpenedNodeGuard>
  );
}

function NodeGroupContent({ active, groupId, navigationActions, node, qualifiedPath, mode, canWriteActiveSpace, canClose, canOpenInNewGroup, onClose, onSetMode, onOpenNodeInNewGroup, onOpenMarkdownLink, onDownloadFile, onRenameNode, onMoveNode, onDeleteNode, onHeaderContextMenu }: NodeActions & EditorNavigationActions & { active: boolean; groupId: number; navigationActions: ReactNode; node: RestNode; qualifiedPath: string | null; mode: "preview" | "edit"; canWriteActiveSpace: boolean; canClose: boolean; canOpenInNewGroup: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void; onDownloadFile: (node: NodeSummary) => void; onHeaderContextMenu: (node: RestNode, event: MouseEvent) => void }) {
  if (node.kind === "text") {
    return <TextEditorView active={active} groupId={groupId} navigationActions={navigationActions} node={node} latestNode={node} qualifiedPath={qualifiedPath} mode={mode} canWriteActiveSpace={canWriteActiveSpace} canOpenInNewGroup={canOpenInNewGroup} canClose={canClose} onClose={onClose} onSetMode={onSetMode} onOpenNodeInNewGroup={onOpenNodeInNewGroup} onOpenMarkdownLink={onOpenMarkdownLink} onRenameNode={onRenameNode} onMoveNode={onMoveNode} onDeleteNode={onDeleteNode} />;
  }
  const Icon = nodeIcon(node);
  return (
    <>
      <EditorGroupHeader
        active={active}
        title={node.name}
        icon={<Icon size={17} />}
        navigationActions={navigationActions}
        qualifiedPath={qualifiedPath}
        canClose={canClose}
        onClose={onClose}
        onContextMenu={(event) => onHeaderContextMenu(node, event)}
        actions={(
          <>
            {node.kind === "file" ? (
              <IconButton label="Download" size="sm" onClick={() => onDownloadFile(node)}>
                <Download size={15} />
              </IconButton>
            ) : null}
            <NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={!canMutateNode(node, canWriteActiveSpace)} />
          </>
        )}
      />
      {node.kind === "file" ? <FileDetailView node={node} /> : <FolderDetailView node={node} />}
    </>
  );
}
