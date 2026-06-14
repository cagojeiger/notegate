import { Database, Folder } from "lucide-react";

import type { RestNode, Space } from "../../api/types";
import type { EditorGroup } from "../../stores/uiStore";
import { EditorGroupHeader } from "./EditorGroupHeader";
import { EmptyEditor } from "./EmptyEditor";
import { FileDetailView } from "./FileDetailView";
import { FolderDetailView } from "./FolderDetailView";
import { NodeActionMenu } from "./NodeActionMenu";
import { TextEditorView } from "./TextEditorView";
import type { NodeActions } from "./types";

type EditorAreaProps = NodeActions & {
  groups: EditorGroup[];
  activeGroupIndex: number;
  activeSpace: Space | null;
  onFocusGroup: (index: number) => void;
  onCloseGroup: (index: number) => void;
  onSetGroupMode: (index: number, mode: "preview" | "edit") => void;
  onCreateFolder: () => void;
  onCreateText: () => void;
  onFileSelected: (file: File | null) => void;
};

export function EditorArea({ groups, activeGroupIndex, activeSpace, onFocusGroup, onCloseGroup, onSetGroupMode, onCreateFolder, onCreateText, onFileSelected, onRenameNode, onMoveNode, onDeleteNode }: EditorAreaProps) {
  const multiple = groups.length > 1;
  return (
    <div className="flex min-w-0 flex-1">
      {groups.map((group, index) => {
        const active = index === activeGroupIndex;
        return (
          <section
            key={group.id}
            data-editor-group
            data-active={active ? "true" : "false"}
            onMouseDown={() => onFocusGroup(index)}
            className={`flex min-w-0 flex-1 flex-col bg-[var(--ng-editor)] ${index > 0 ? "border-l border-seam" : ""} ${active ? "" : "max-md:hidden"} ${multiple ? (active ? "relative z-10 outline outline-1 -outline-offset-1 outline-[var(--ng-active-border)]" : "outline outline-1 -outline-offset-1 outline-transparent") : ""}`}
          >
            <GroupBody
              active={active}
              node={group.node}
              mode={group.mode}
              activeSpace={activeSpace}
              canClose={multiple}
              onClose={() => onCloseGroup(index)}
              onSetMode={(mode) => onSetGroupMode(index, mode)}
              onCreateFolder={onCreateFolder}
              onCreateText={onCreateText}
              onFileSelected={onFileSelected}
              onRenameNode={onRenameNode}
              onMoveNode={onMoveNode}
              onDeleteNode={onDeleteNode}
            />
          </section>
        );
      })}
    </div>
  );
}

function GroupBody({ active, node, mode, activeSpace, canClose, onClose, onSetMode, onCreateFolder, onCreateText, onFileSelected, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & { active: boolean; node: RestNode | null; mode: "preview" | "edit"; activeSpace: Space | null; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void }) {
  if (!node) {
    return (
      <>
        <EditorGroupHeader title="Open a node" canClose={canClose} onClose={onClose} active={active} />
        <EmptyEditor activeSpace={activeSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} />
      </>
    );
  }
  if (node.kind === "text") {
    return <TextEditorView active={active} node={node} mode={mode} canClose={canClose} onClose={onClose} onSetMode={onSetMode} onRenameNode={onRenameNode} onMoveNode={onMoveNode} onDeleteNode={onDeleteNode} />;
  }
  const Icon = node.kind === "file" ? Database : Folder;
  return (
    <>
      <EditorGroupHeader
        active={active}
        title={node.name}
        icon={<Icon size={17} />}
        canClose={canClose}
        onClose={onClose}
        actions={<NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null} />}
      />
      {node.kind === "file" ? <FileDetailView node={node} /> : <FolderDetailView node={node} />}
    </>
  );
}
