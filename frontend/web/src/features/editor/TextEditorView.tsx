import { ChevronsDownUp, ChevronsUpDown, FileText } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { RestNode } from "../../api/types";
import { Button, IconButton } from "../../shared/ui";
import { EditorGroupHeader } from "./EditorGroupHeader";
import { NodeActionMenu } from "./NodeActionMenu";
import { TextPreview } from "./TextPreview";
import { inferTextFormat, isStructuredFormat } from "./textFormat";
import type { StructuredPreviewMode } from "./StructuredPreview";
import type { StructuredExpansionMode } from "./StructuredTreeView";
import type { NodeActions } from "./types";
import { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

export function TextEditorView({ active, node, mode, canClose, onClose, onSetMode, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & { active: boolean; node: RestNode; mode: "preview" | "edit"; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void }) {
  const textQuery = useTextDocument(node);
  const [draft, setDraft] = useState("");
  const [conflict, setConflict] = useState(false);
  const [structuredMode, setStructuredMode] = useState<StructuredPreviewMode>("tree");
  const [structuredExpansionMode, setStructuredExpansionMode] = useState<StructuredExpansionMode>("expanded");
  const text = textQuery.data?.text;
  const content = text && "content" in text ? text.content : "";
  const sha = text && "content_sha256" in text ? text.content_sha256 : undefined;
  const encrypted = !!text && "encrypted_payload" in text;
  const format = inferTextFormat(node.name);
  const structured = isStructuredFormat(format);
  const prevMode = useRef<"preview" | "edit">(mode);

  useEffect(() => {
    setStructuredMode("tree");
    setStructuredExpansionMode("expanded");
  }, [node.id]);

  useEffect(() => {
    // Load the editor draft from the loaded content when entering edit mode.
    if (mode === "edit" && prevMode.current !== "edit") setDraft(content);
    prevMode.current = mode;
  }, [mode, content]);
  const dirty = mode === "edit" && draft !== content;
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
      {mode === "edit" ? <Button size="xs" onClick={() => saveMutation.mutate(false)} disabled={saveMutation.isPending || !dirty}>Save</Button> : null}
      <Button size="xs" secondary onClick={() => onSetMode(mode === "edit" ? "preview" : "edit")} disabled={encrypted}>{mode === "edit" ? "Preview" : "Edit"}</Button>
      <NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null} />
    </>
  );
  return (
    <>
      <EditorGroupHeader active={active} title={node.name} icon={<FileText size={17} />} titleActions={titleActions} actions={actions} canClose={canClose} onClose={onClose} dirty={dirty} />
      {textQuery.isLoading ? (
        <div className="p-10 text-muted">Loading text…</div>
      ) : textQuery.isError ? (
        <div className="p-10 text-danger">Could not load text.</div>
      ) : encrypted ? (
        <div className="p-10 text-muted">Encrypted text cannot be previewed by the server.</div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          {conflict ? (
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning">
              <span>This node changed elsewhere since you opened it.</span>
              <div className="flex gap-2">
                <Button size="sm" secondary onClick={() => { void textQuery.refetch(); setConflict(false); }}>Reload</Button>
                <Button size="sm" variant="danger" onClick={() => saveMutation.mutate(true)}>Overwrite</Button>
              </div>
            </div>
          ) : null}
          {mode === "edit" ? (
            <LineNumberedTextArea value={draft} onChange={setDraft} />
          ) : (
            <TextPreview name={node.name} content={content} structuredMode={structuredMode} structuredExpansionMode={structuredExpansionMode} />
          )}
        </div>
      )}
    </>
  );
}

function LineNumberedTextArea({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const gutterRef = useRef<HTMLDivElement | null>(null);
  const lineCount = Math.max(1, value.split("\n").length);

  return (
    <div className="flex min-h-0 flex-1 bg-[var(--ng-editor)] font-mono text-sm leading-6 text-text">
      <div ref={gutterRef} className="select-none overflow-hidden border-r border-seam px-4 py-8 text-right text-faint" aria-hidden="true">
        {Array.from({ length: lineCount }, (_, index) => (
          <div key={index} className="h-6 tabular-nums">{index + 1}</div>
        ))}
      </div>
      <textarea
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
