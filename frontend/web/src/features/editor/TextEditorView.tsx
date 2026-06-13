import { FileText } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { RestNode } from "../../api/types";
import { Button } from "../../shared/ui";
import { Markdown } from "../../shared/ui/Markdown";
import { EditorGroupHeader } from "./EditorGroupHeader";
import { NodeActionMenu } from "./NodeActionMenu";
import type { NodeActions } from "./types";
import { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

export function TextEditorView({ node, mode, canClose, onClose, onSetMode, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & { node: RestNode; mode: "preview" | "edit"; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void }) {
  const textQuery = useTextDocument(node);
  const [draft, setDraft] = useState("");
  const [conflict, setConflict] = useState(false);
  const text = textQuery.data?.text;
  const content = text && "content" in text ? text.content : "";
  const sha = text && "content_sha256" in text ? text.content_sha256 : undefined;
  const encrypted = !!text && "encrypted_payload" in text;
  const prevMode = useRef<"preview" | "edit">(mode);
  useEffect(() => {
    // Load the editor draft from the loaded content when entering edit mode.
    if (mode === "edit" && prevMode.current !== "edit") setDraft(content);
    prevMode.current = mode;
  }, [mode, content]);
  const dirty = mode === "edit" && draft !== content;
  const isMarkdown = /\.(md|markdown)$/i.test(node.name);
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
  const actions = (
    <>
      {mode === "edit" ? <Button size="xs" onClick={() => saveMutation.mutate(false)} disabled={saveMutation.isPending || !dirty}>Save</Button> : null}
      <Button size="xs" secondary onClick={() => onSetMode(mode === "edit" ? "preview" : "edit")} disabled={encrypted}>{mode === "edit" ? "Preview" : "Edit"}</Button>
      <NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null} />
    </>
  );
  return (
    <>
      <EditorGroupHeader title={node.name} icon={<FileText size={17} />} actions={actions} canClose={canClose} onClose={onClose} dirty={dirty} />
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
            <textarea className="min-h-0 flex-1 resize-none bg-[var(--ng-editor)] p-8 font-mono text-sm text-text outline-none" value={draft} onChange={(event) => setDraft(event.target.value)} />
          ) : isMarkdown ? (
            <div className="mx-auto w-full max-w-[44rem] overflow-y-auto px-10 py-14"><Markdown content={content} /></div>
          ) : (
            <article className="mx-auto max-w-[44rem] overflow-y-auto whitespace-pre-wrap px-10 py-14 font-mono text-sm text-text">{content}</article>
          )}
        </div>
      )}
    </>
  );
}
