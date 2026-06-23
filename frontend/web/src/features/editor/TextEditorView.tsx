import { ChevronsDownUp, ChevronsUpDown, Copy, FileText, Move, PanelRightOpen, Pencil, Save, Trash2, Undo2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import { Button, Card, IconButton, MenuButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";
import { EditorGroupHeader } from "./EditorGroupHeader";
import { NodeActionMenu } from "./NodeActionMenu";
import { TextPreview } from "./TextPreview";
import { inferTextFormat, isStructuredFormat } from "./textFormat";
import type { StructuredPreviewMode } from "./StructuredPreview";
import type { StructuredExpansionMode } from "./StructuredTreeView";
import type { EditorNavigationActions, NodeActions } from "./types";
import { useMarkdownImageLoader, useSaveTextDocument, useTextDocument } from "./useEditorQueries";
import { useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

export function TextEditorView({ active, groupId, node, latestNode, mode, canWriteActiveSpace, canOpenInNewGroup, canClose, onClose, onSetMode, onOpenNodeInNewGroup, onOpenMarkdownLink, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & EditorNavigationActions & { active: boolean; groupId: number; node: RestNode; latestNode?: RestNode; mode: "preview" | "edit"; canWriteActiveSpace: boolean; canOpenInNewGroup: boolean; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void }) {
  const textQuery = useTextDocument(node);
  const loadMarkdownImage = useMarkdownImageLoader(node);
  const [draft, setDraft] = useState("");
  const [conflict, setConflict] = useState(false);
  const [externalUpdate, setExternalUpdate] = useState<RestNode | null>(null);
  const [editorMenu, setEditorMenu] = useState<{ x: number; y: number } | null>(null);
  const lastAutoReloadSha = useRef<string | null>(null);
  const dismissedExternalSha = useRef<string | null>(null);
  const [structuredMode, setStructuredMode] = useState<StructuredPreviewMode>("tree");
  const [structuredExpansionMode, setStructuredExpansionMode] = useState<StructuredExpansionMode>("expanded");
  const text = textQuery.data?.text;
  const plainText = isPlainTextContent(text) ? text : null;
  const content = plainText?.content ?? "";
  const sha = text?.content_sha256;
  const encrypted = isEncryptedTextContent(text);
  const partialText = plainText?.truncated ? plainText : null;
  const canEditText = canWriteActiveSpace && !!plainText && !partialText;
  const canCopyContent = !!plainText && !partialText;
  const copySource = mode === "edit" && canEditText ? draft : content;
  const format = inferTextFormat(node.name);
  const structured = isStructuredFormat(format);
  const prevMode = useRef<"preview" | "edit">(mode);
  const showToast = useUiStore((state) => state.showToast);
  const markdownLinkPolicy = useMemo(
    () => ({
      sourcePath: node.path,
      onOpenInternalLink: (path: string) => onOpenMarkdownLink(groupId, node, path),
      onInvalidInternalLink: () => showToast("Invalid markdown link")
    }),
    [groupId, node, onOpenMarkdownLink, showToast]
  );
  const markdownImagePolicy = useMemo(
    () => ({
      sourcePath: node.path,
      loadInternalImage: loadMarkdownImage
    }),
    [loadMarkdownImage, node.path]
  );

  useEffect(() => {
    setStructuredMode("tree");
    setStructuredExpansionMode("expanded");
    setExternalUpdate(null);
    setEditorMenu(null);
    lastAutoReloadSha.current = null;
    dismissedExternalSha.current = null;
  }, [node.id]);

  useEffect(() => {
    // Load the editor draft from the loaded content when entering edit mode.
    if (mode === "edit" && canEditText && prevMode.current !== "edit") setDraft(content);
    prevMode.current = mode;
  }, [mode, content, canEditText]);

  useEffect(() => {
    if (mode === "edit" && textQuery.isSuccess && !canEditText) onSetMode("preview");
  }, [canEditText, mode, onSetMode, textQuery.isSuccess]);

  const dirty = mode === "edit" && canEditText && draft !== content;

  function openEditorMenu(event: MouseEvent) {
    event.preventDefault();
    setEditorMenu({ x: event.clientX, y: event.clientY });
  }

  async function copyContent() {
    showToast((await copyText(copySource)) ? "Content copied" : "Could not copy content");
  }

  async function copyPath() {
    showToast((await copyText(node.path)) ? "Path copied" : "Could not copy path");
  }

  function saveDraft() {
    saveMutation.mutate(false);
  }

  function editText() {
    onSetMode("edit");
  }

  function cancelEdit() {
    if (dirty) {
      setDraft(content);
      showToast("Edit canceled");
    }
    setConflict(false);
    setExternalUpdate(null);
    onSetMode("preview");
  }

  async function reloadLatestText() {
    const result = await textQuery.refetch();
    const nextText = result.data?.text;
    if (isPlainTextContent(nextText)) setDraft(nextText.content);
    setExternalUpdate(null);
    dismissedExternalSha.current = null;
  }

  useEffect(() => {
    const latestSha = latestNode?.content_sha256;
    if (!latestNode || !latestSha || !sha) return;

    if (latestSha === sha) {
      if (externalUpdate?.content_sha256 === latestSha) setExternalUpdate(null);
      lastAutoReloadSha.current = null;
      return;
    }

    if (dirty) {
      if (dismissedExternalSha.current !== latestSha) setExternalUpdate(latestNode);
      return;
    }

    if (lastAutoReloadSha.current === latestSha) return;
    lastAutoReloadSha.current = latestSha;
    void reloadLatestText();
  }, [dirty, externalUpdate?.content_sha256, latestNode, sha]);

  const saveMutation = useSaveTextDocument(
    node,
    draft,
    sha,
    () => {
      setConflict(false);
      onSetMode("preview");
    },
    () => setConflict(true)
  );
  const canSave = mode === "edit" && dirty && !saveMutation.isPending;
  const titleActions = mode === "preview" && structured && !encrypted ? (
    <>
      <IconButton label="Expand all" size="sm" onClick={() => setStructuredExpansionMode("expanded")} disabled={structuredMode !== "tree"}>
        <ChevronsUpDown size={14} />
      </IconButton>
      <IconButton label="Collapse all" size="sm" onClick={() => setStructuredExpansionMode("collapsed")} disabled={structuredMode !== "tree"}>
        <ChevronsDownUp size={14} />
      </IconButton>
    </>
  ) : null;
  const actions = (
    <>
      {mode === "preview" && structured && !encrypted ? (
        <>
          <Button size="xs" variant={structuredMode === "tree" ? "primary" : "secondary"} onClick={() => setStructuredMode("tree")}>Tree</Button>
          <Button size="xs" variant={structuredMode === "source" ? "primary" : "secondary"} onClick={() => setStructuredMode("source")}>Source</Button>
        </>
      ) : null}
      <IconButton label="Copy content" size="sm" onClick={() => { void copyContent(); }} disabled={!canCopyContent}>
        <Copy size={15} />
      </IconButton>
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
      <NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null || !canWriteActiveSpace} />
    </>
  );
  return (
    <>
      <EditorGroupHeader active={active} title={node.name} icon={<FileText size={17} />} titleActions={titleActions} actions={actions} canClose={canClose} onClose={onClose} onContextMenu={openEditorMenu} dirty={dirty} />
      {textQuery.isLoading ? (
        <div className="p-10 text-muted">Loading text…</div>
      ) : textQuery.isError ? (
        <div className="p-10 text-danger">Could not load text.</div>
      ) : encrypted ? (
        <div className="p-10 text-muted">Encrypted text cannot be previewed by the server.</div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col" onContextMenu={mode === "preview" ? openEditorMenu : undefined}>
          {partialText ? (
            <div className="border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              Loaded {partialText.returned_lines} of {partialText.line_count} lines. Editing is disabled until the full document is available.
            </div>
          ) : null}
          {conflict ? (
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              <span>This node changed elsewhere since you opened it.</span>
              <div className="flex gap-2">
                <Button size="sm" secondary onClick={() => { void reloadLatestText(); setConflict(false); }}>Reload</Button>
                <Button size="sm" variant="danger" onClick={() => saveMutation.mutate(true)}>Overwrite</Button>
              </div>
            </div>
          ) : null}
          {externalUpdate ? (
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              <span>This document changed outside this editor.</span>
              <div className="flex gap-2">
                <Button size="sm" secondary onClick={() => { void reloadLatestText(); onSetMode("preview"); }}>Reload latest</Button>
                <Button size="sm" secondary onClick={() => { dismissedExternalSha.current = externalUpdate.content_sha256 ?? null; setExternalUpdate(null); }}>Keep editing</Button>
              </div>
            </div>
          ) : null}
          {mode === "edit" ? (
            <LineNumberedTextArea value={draft} onChange={setDraft} />
          ) : (
            <TextPreview
              name={node.name}
              content={content}
              markdownLinkPolicy={markdownLinkPolicy}
              markdownImagePolicy={markdownImagePolicy}
              structuredMode={structuredMode}
              structuredExpansionMode={structuredExpansionMode}
            />
          )}
        </div>
      )}
      {editorMenu ? (
        <EditorContextMenu
          menu={editorMenu}
          node={node}
          mode={mode}
          canCopyContent={canCopyContent}
          canEditText={canEditText}
          canSave={canSave}
          canMutateNode={node.parent_id !== null && canWriteActiveSpace}
          canOpenInNewGroup={canOpenInNewGroup}
          canCloseGroup={canClose}
          onClose={() => setEditorMenu(null)}
          onCopyContent={() => { void copyContent(); }}
          onEditText={editText}
          onSaveDraft={saveDraft}
          onCancelEdit={cancelEdit}
          onOpenInNewGroup={() => onOpenNodeInNewGroup(node)}
          onCopyPath={() => { void copyPath(); }}
          onCloseGroup={onClose}
          onRenameNode={() => onRenameNode(node)}
          onMoveNode={() => onMoveNode(node)}
          onDeleteNode={() => onDeleteNode(node)}
        />
      ) : null}
    </>
  );
}

function EditorContextMenu({
  menu,
  node,
  mode,
  canCopyContent,
  canEditText,
  canSave,
  canMutateNode,
  canOpenInNewGroup,
  canCloseGroup,
  onClose,
  onCopyContent,
  onEditText,
  onSaveDraft,
  onCancelEdit,
  onOpenInNewGroup,
  onCopyPath,
  onCloseGroup,
  onRenameNode,
  onMoveNode,
  onDeleteNode
}: {
  menu: { x: number; y: number };
  node: RestNode;
  mode: "preview" | "edit";
  canCopyContent: boolean;
  canEditText: boolean;
  canSave: boolean;
  canMutateNode: boolean;
  canOpenInNewGroup: boolean;
  canCloseGroup: boolean;
  onClose: () => void;
  onCopyContent: () => void;
  onEditText: () => void;
  onSaveDraft: () => void;
  onCancelEdit: () => void;
  onOpenInNewGroup: () => void;
  onCopyPath: () => void;
  onCloseGroup: () => void;
  onRenameNode: () => void;
  onMoveNode: () => void;
  onDeleteNode: () => void;
}) {
  const menuWidth = 208;
  const menuHeight = mode === "edit" ? 332 : 296;

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function run(action: () => void) {
    action();
    onClose();
  }

  const left = Math.max(8, Math.min(menu.x, window.innerWidth - menuWidth - 8));
  const top = Math.max(8, Math.min(menu.y, window.innerHeight - menuHeight - 8));
  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <Card className="fixed z-50 w-52 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none" style={{ left, top }} role="menu">
        <div className="truncate px-3 py-1 text-xs text-muted">{node.name}</div>
        <MenuButton onClick={() => run(onCopyContent)} disabled={!canCopyContent}><Copy size={14} /> Copy content</MenuButton>
        {mode === "edit" ? (
          <>
            <MenuButton onClick={() => run(onSaveDraft)} disabled={!canSave}><Save size={14} /> Save</MenuButton>
            <MenuButton onClick={() => run(onCancelEdit)}><Undo2 size={14} /> Cancel edit</MenuButton>
          </>
        ) : (
          <MenuButton onClick={() => run(onEditText)} disabled={!canEditText}><Pencil size={14} /> Edit</MenuButton>
        )}
        <MenuButton onClick={() => run(onOpenInNewGroup)} disabled={!canOpenInNewGroup}><PanelRightOpen size={14} /> Open in new group</MenuButton>
        <MenuButton onClick={() => run(onCopyPath)}><Copy size={14} /> Copy path</MenuButton>
        {canCloseGroup ? <MenuButton onClick={() => run(onCloseGroup)}><X size={14} /> Close group</MenuButton> : null}
        <div className="my-1 border-t border-border" />
        <MenuButton onClick={() => run(onRenameNode)} disabled={!canMutateNode}><Pencil size={14} /> Rename</MenuButton>
        <MenuButton onClick={() => run(onMoveNode)} disabled={!canMutateNode}><Move size={14} /> Move…</MenuButton>
        <MenuButton danger onClick={() => run(onDeleteNode)} disabled={!canMutateNode}><Trash2 size={14} /> Delete</MenuButton>
      </Card>
    </>
  );
}

type TextContent = ReadTextResponse["text"];

function isPlainTextContent(text: TextContent | undefined): text is Extract<TextContent, { storage_format: "plain" }> {
  return !!text && text.storage_format === "plain" && "content" in text;
}

function isEncryptedTextContent(text: TextContent | undefined): text is Extract<TextContent, { storage_format: "encrypted" }> {
  return !!text && text.storage_format === "encrypted" && "encrypted_payload" in text;
}

function LineNumberedTextArea({ value, onChange }: { value: string; onChange: (value: string) => void }) {
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
        wrap="off"
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
