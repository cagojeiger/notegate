import { ChevronsDownUp, ChevronsUpDown, Copy, FileText, MoreHorizontal, Pencil, Save, Undo2 } from "lucide-react";
import { lazy, Suspense, useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";

import type { RestNode } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import { Button, IconButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { canMutateNode } from "../nodes/nodeWriteAccess";
import { EditorGroupHeader } from "./EditorGroupHeader";
import type { MarkdownOutlineIdentity } from "./MarkdownOutlineContext";
import { TextPreview } from "./TextPreview";
import { inferTextFormat, isStructuredFormat, isTabularFormat } from "./textFormat";
import type { StructuredExpansionMode } from "./StructuredTreeView";
import type { EditorNavigationActions, NodeActions } from "./types";
import { useMarkdownImageLoader } from "./useFilePreviewQueries";
import { useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";
import { useTextEditorSession } from "./useTextEditorSession";

const EditorContextMenu = lazy(() => import("./EditorContextMenu"));

export function TextEditorView({ active, groupId, navigationActions, node, latestNode, qualifiedPath, mode, canWriteActiveSpace, canOpenInNewGroup, canClose, onClose, onSetMode, onOpenNodeInNewGroup, onOpenMarkdownLink, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & EditorNavigationActions & { active: boolean; groupId: number; navigationActions?: ReactNode; node: RestNode; latestNode?: RestNode; qualifiedPath: string | null; mode: "preview" | "edit"; canWriteActiveSpace: boolean; canOpenInNewGroup: boolean; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void }) {
  const loadMarkdownImage = useMarkdownImageLoader(node);
  const [editorMenu, setEditorMenu] = useState<{ x: number; y: number } | null>(null);
  const [sourceView, setSourceView] = useState(false);
  const [structuredExpansionMode, setStructuredExpansionMode] = useState<StructuredExpansionMode>("expanded");
  const {
    textQuery,
    content,
    draft,
    setDraft,
    encrypted,
    partialText,
    canEdit: canEditText,
    canCopy: canCopyContent,
    dirty,
    conflict,
    externalUpdate,
    canSave,
    saveDraft,
    overwriteDraft,
    cancelEdit,
    reloadConflict,
    reloadExternalUpdate,
    dismissExternalUpdate
  } = useTextEditorSession({ node, latestNode, mode, canWrite: canWriteActiveSpace, onSetMode });
  const copySource = mode === "edit" ? draft : content;
  const format = inferTextFormat(node.name);
  const structured = isStructuredFormat(format);
  const tabular = isTabularFormat(format);
  const visualPreview = structured || tabular;
  const sourceOnly = tabular && Boolean(partialText);
  const showSource = sourceView || sourceOnly;
  const showToast = useUiStore((state) => state.showToast);
  const editorActionsRef = useRef<HTMLDivElement>(null);
  const openMarkdownLinkRef = useRef(onOpenMarkdownLink);
  useLayoutEffect(() => {
    openMarkdownLinkRef.current = onOpenMarkdownLink;
  }, [onOpenMarkdownLink]);
  const markdownLinkPolicy = useMemo(
    () => ({
      sourcePath: node.path,
      onOpenInternalLink: (path: string) => openMarkdownLinkRef.current(groupId, node, path),
      onInvalidInternalLink: () => showToast("Invalid markdown link")
    }),
    [groupId, node, showToast]
  );
  const markdownImagePolicy = useMemo(
    () => ({
      sourcePath: node.path,
      loadInternalImage: loadMarkdownImage
    }),
    [loadMarkdownImage, node.path]
  );
  const markdownOutlineIdentity = useMemo<MarkdownOutlineIdentity>(() => ({
    groupId,
    spaceId: node.space_id,
    nodeId: node.id
  }), [groupId, node.id, node.space_id]);

  useEffect(() => {
    setSourceView(false);
    setStructuredExpansionMode("expanded");
    setEditorMenu(null);
  }, [node.id]);

  function openEditorMenu(event: MouseEvent) {
    event.preventDefault();
    setEditorMenu({ x: event.clientX, y: event.clientY });
  }

  function openEditorActions() {
    const bounds = editorActionsRef.current?.getBoundingClientRect();
    if (!bounds) return;
    setEditorMenu({ x: bounds.right, y: bounds.bottom + 4 });
  }

  async function copyContent() {
    showToast((await copyText(copySource)) ? "Content copied" : "Could not copy content");
  }

  async function copyPath() {
    if (!qualifiedPath) return;
    showToast((await copyText(qualifiedPath)) ? "Path copied" : "Could not copy path");
  }

  function editText() {
    onSetMode("edit");
  }
  const titleActions = mode === "preview" && structured && !encrypted ? (
    <>
      <IconButton label="Expand all" size="sm" onClick={() => setStructuredExpansionMode("expanded")} disabled={showSource}>
        <ChevronsUpDown size={14} />
      </IconButton>
      <IconButton label="Collapse all" size="sm" onClick={() => setStructuredExpansionMode("collapsed")} disabled={showSource}>
        <ChevronsDownUp size={14} />
      </IconButton>
    </>
  ) : null;
  const actions = (
    <>
      {mode === "preview" && visualPreview && !encrypted ? (
        <>
          <Button size="xs" variant={!showSource ? "primary" : "secondary"} aria-pressed={!showSource} onClick={() => setSourceView(false)} disabled={sourceOnly}>{structured ? "Tree" : "Table"}</Button>
          <Button size="xs" variant={showSource ? "primary" : "secondary"} aria-pressed={showSource} onClick={() => setSourceView(true)}>Source</Button>
        </>
      ) : null}
      <span className="max-md:hidden">
        <IconButton label="Copy content" size="sm" onClick={() => { void copyContent(); }} disabled={!canCopyContent}>
          <Copy size={15} />
        </IconButton>
      </span>
      {mode === "edit" ? (
        <>
          <IconButton label="Save" size="sm" onClick={saveDraft} disabled={!canSave}>
            <Save size={15} />
          </IconButton>
          <IconButton label="Cancel edit" size="sm" onClick={cancelEdit}>
            <Undo2 size={15} />
          </IconButton>
        </>
      ) : (
        <IconButton label="Edit" size="sm" onClick={editText} disabled={!canEditText}>
          <Pencil size={15} />
        </IconButton>
      )}
      <div ref={editorActionsRef}>
        <IconButton label="More actions" expanded={Boolean(editorMenu)} hasPopup="menu" onClick={openEditorActions}>
          <MoreHorizontal size={16} />
        </IconButton>
      </div>
    </>
  );
  return (
    <>
      <EditorGroupHeader active={active} title={node.name} icon={<FileText size={17} />} navigationActions={navigationActions} qualifiedPath={qualifiedPath} titleActions={titleActions} actions={actions} canClose={canClose} onClose={onClose} onContextMenu={openEditorMenu} dirty={dirty} collapseSecondaryActions />
      {textQuery.isLoading ? (
        <div className="p-10 text-muted">Loading text…</div>
      ) : textQuery.isError ? (
        <div className="p-10 text-danger">Could not load text.</div>
      ) : encrypted ? (
        <div className="p-10 text-muted">Encrypted text cannot be previewed by the server.</div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col" onContextMenu={openEditorMenu}>
          {partialText ? (
            <div className="border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              Loaded {partialText.returned_lines} of {partialText.line_count} lines. Editing is disabled until the full document is available.
            </div>
          ) : null}
          {mode === "edit" && node.effective_write_locked ? (
            <div className="border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              This document is now locked. Unsaved edits are preserved here but cannot be saved.
            </div>
          ) : null}
          {conflict ? (
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              <span>This document changed elsewhere since you opened it.</span>
              <div className="flex gap-2">
                <Button size="sm" secondary onClick={reloadConflict}>Reload</Button>
                <Button size="sm" variant="danger" onClick={overwriteDraft}>Overwrite</Button>
              </div>
            </div>
          ) : null}
          {externalUpdate ? (
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              <span>This document changed outside this editor.</span>
              <div className="flex gap-2">
                <Button size="sm" secondary onClick={reloadExternalUpdate}>Reload latest</Button>
                <Button size="sm" secondary onClick={dismissExternalUpdate}>Keep editing</Button>
              </div>
            </div>
          ) : null}
          {mode === "edit" ? (
            <LineNumberedTextArea
              value={draft}
              readOnly={node.effective_write_locked}
              onChange={setDraft}
            />
          ) : (
            <TextPreview
              name={node.name}
              content={content}
              previewIdentity={node.id}
              markdownLinkPolicy={markdownLinkPolicy}
              markdownImagePolicy={markdownImagePolicy}
              markdownOutlineIdentity={markdownOutlineIdentity}
              structuredMode={showSource ? "source" : "tree"}
              structuredExpansionMode={structuredExpansionMode}
              tabularMode={showSource ? "source" : "table"}
            />
          )}
        </div>
      )}
      {editorMenu ? (
        <Suspense fallback={null}>
          <EditorContextMenu
            menu={editorMenu}
            node={node}
            mode={mode}
            canCopyContent={canCopyContent}
            canEditText={canEditText}
            canSave={canSave}
            canCopyPath={Boolean(qualifiedPath)}
            canMutateNode={canMutateNode(node, canWriteActiveSpace)}
            canOpenInNewGroup={canOpenInNewGroup}
            canCloseGroup={canClose}
            showStructuredActions={mode === "preview" && structured && !encrypted}
            structuredActionsDisabled={showSource}
            onClose={() => setEditorMenu(null)}
            onCopyContent={() => { void copyContent(); }}
            onEditText={editText}
            onSaveDraft={saveDraft}
            onCancelEdit={cancelEdit}
            onOpenInNewGroup={() => onOpenNodeInNewGroup(node)}
            onCopyPath={() => { void copyPath(); }}
            onCloseGroup={onClose}
            onExpandAll={() => setStructuredExpansionMode("expanded")}
            onCollapseAll={() => setStructuredExpansionMode("collapsed")}
            onRenameNode={() => onRenameNode(node)}
            onMoveNode={() => onMoveNode(node)}
            onDeleteNode={() => onDeleteNode(node)}
          />
        </Suspense>
      ) : null}
    </>
  );
}

function LineNumberedTextArea({
  value,
  readOnly = false,
  onChange
}: {
  value: string;
  readOnly?: boolean;
  onChange: (value: string) => void;
}) {
  const gutterRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const lineCount = Math.max(1, value.split("\n").length);

  useResetHorizontalScrollOnGrow(textareaRef);

  return (
    <div className="flex min-h-0 flex-1 bg-[var(--ng-editor)] font-mono text-sm leading-6 text-text">
      <div ref={gutterRef} className="select-none overflow-hidden border-r border-seam px-4 py-8 text-right text-faint" aria-hidden="true">
        {Array.from({ length: lineCount }, (_, index) => (
          <div key={index} className="h-6 tabular-nums">{index + 1}</div>
        ))}
      </div>
      <textarea
        ref={textareaRef}
        aria-label="Edit text content"
        readOnly={readOnly}
        wrap="off"
        onContextMenu={(event) => event.stopPropagation()}
        className="min-h-0 flex-1 resize-none overflow-auto bg-transparent px-5 py-8 font-mono text-sm leading-6 text-text outline-none"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onScroll={(event) => {
          if (gutterRef.current) gutterRef.current.scrollTop = event.currentTarget.scrollTop;
        }}
      />
    </div>
  );
}
