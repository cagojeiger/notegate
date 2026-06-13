import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database, Download, FileText, Folder, MoreHorizontal, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { downloadFile } from "../../api/files";
import { listChildren } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { RestNode, Space } from "../../api/types";
import type { EditorGroup } from "../../stores/uiStore";
import { useUiStore } from "../../stores/uiStore";
import { Markdown } from "../../shared/ui/Markdown";
import { Button, IconButton, MenuButton } from "../../shared/ui";

type NodeActions = {
  onRenameNode: (node: RestNode) => void;
  onMoveNode: (node: RestNode) => void;
  onDeleteNode: (node: RestNode) => void;
};

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
            onMouseDown={() => onFocusGroup(index)}
            className={`flex min-w-0 flex-1 flex-col bg-bg ${index > 0 ? "border-l border-seam" : ""} ${active ? "" : "max-md:hidden"} ${multiple && active ? "ring-1 ring-inset ring-primary/40" : ""}`}
          >
            <GroupBody
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

function GroupBody({ node, mode, activeSpace, canClose, onClose, onSetMode, onCreateFolder, onCreateText, onFileSelected, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & { node: RestNode | null; mode: "preview" | "edit"; activeSpace: Space | null; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void }) {
  if (!node) {
    return (
      <>
        <GroupHeader title="Open a node" canClose={canClose} onClose={onClose} />
        <EmptyEditor activeSpace={activeSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} />
      </>
    );
  }
  if (node.kind === "text") {
    return <TextGroup node={node} mode={mode} canClose={canClose} onClose={onClose} onSetMode={onSetMode} onRenameNode={onRenameNode} onMoveNode={onMoveNode} onDeleteNode={onDeleteNode} />;
  }
  const Icon = node.kind === "file" ? Database : Folder;
  return (
    <>
      <GroupHeader
        title={node.name}
        icon={<Icon size={17} />}
        canClose={canClose}
        onClose={onClose}
        actions={<NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null} />}
      />
      {node.kind === "file" ? <FileView node={node} /> : <FolderView node={node} />}
    </>
  );
}

function GroupHeader({ title, icon, actions, canClose, onClose, dirty }: { title: string; icon?: ReactNode; actions?: ReactNode; canClose: boolean; onClose: () => void; dirty?: boolean }) {
  return (
    <div className="flex h-12 items-center justify-between border-b border-seam px-4">
      <div className="flex min-w-0 items-center gap-2 font-semibold">{icon}<span className="truncate">{title}</span>{dirty ? <span className="size-1.5 shrink-0 rounded-full bg-warning" title="Unsaved changes" /> : null}</div>
      <div className="flex items-center gap-1">
        {actions}
        {canClose ? <IconButton label="Close editor group" onClick={onClose}><X size={16} /></IconButton> : null}
      </div>
    </div>
  );
}

function EmptyEditor({ activeSpace, onCreateFolder, onCreateText, onFileSelected }: { activeSpace: Space | null; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void }) {
  return (
    <section className="grid min-w-0 flex-1 place-items-center bg-bg px-6 text-muted">
      <div className="max-w-md text-center">
        <div className="mx-auto mb-5 grid size-14 place-items-center rounded-2xl border border-border bg-surface"><FileText size={26} /></div>
        <div className="text-xl font-semibold text-text">Open a node</div>
        <p className="mt-2 text-sm leading-6">Select an item from Tree or Recent. Create a first item when this space is empty.</p>
        {activeSpace ? (
          <div className="mt-6 flex justify-center gap-2">
            <Button onClick={onCreateText}>New text</Button>
            <Button secondary onClick={onCreateFolder}>New folder</Button>
            <label className="inline-flex cursor-pointer items-center rounded-lg border border-border bg-surface px-3 py-2 text-sm text-muted hover:bg-panel hover:text-text">
              Upload file
              <input className="hidden" type="file" onChange={(event) => onFileSelected(event.target.files?.[0] ?? null)} />
            </label>
          </div>
        ) : null}
      </div>
    </section>
  );
}

function NodeActionMenu({ onRenameNode, onMoveNode, onDeleteNode, disabled }: { onRenameNode: () => void; onMoveNode: () => void; onDeleteNode: () => void; disabled: boolean }) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);
  return (
    <div className="relative">
      <IconButton label="Node actions" onClick={() => setOpen((value) => !value)} disabled={disabled}><MoreHorizontal size={16} /></IconButton>
      {open ? (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} onContextMenu={(event) => { event.preventDefault(); setOpen(false); }} aria-hidden="true" />
          <div className="absolute right-0 top-9 z-20 w-40 rounded-xl border border-border bg-surface p-1 text-sm shadow-[var(--ng-focus-shadow)]">
            <MenuButton onClick={() => { onRenameNode(); setOpen(false); }}>Rename</MenuButton>
            <MenuButton onClick={() => { onMoveNode(); setOpen(false); }}>Move</MenuButton>
            <MenuButton danger onClick={() => { onDeleteNode(); setOpen(false); }}><Trash2 size={14} /> Delete</MenuButton>
          </div>
        </>
      ) : null}
    </div>
  );
}

function FolderView({ node }: { node: RestNode }) {
  const client = useApiClient();
  const childrenQuery = useQuery({ queryKey: [...queryKeys.children(node.space_id, node.id), "stat"], queryFn: () => listChildren(client, node.space_id, node.id) });
  const childCount = childrenQuery.data ? `${childrenQuery.data.children.length}${childrenQuery.data.page.has_more ? "+" : ""}` : "…";
  return (
    <article className="mx-auto max-w-3xl px-10 py-14">
      <p className="text-sm text-muted">{node.path}</p>
      <h1 className="mt-4 text-4xl font-semibold tracking-tight">{node.name}</h1>
      <dl className="mt-8 grid grid-cols-[120px_1fr] gap-y-3 text-sm">
        <dt className="font-semibold">Children</dt><dd className="text-muted">{childCount}</dd>
        <dt className="font-semibold">Updated</dt><dd className="text-muted">{node.updated_at.slice(0, 10)}</dd>
      </dl>
      <p className="mt-8 leading-7 text-muted">Folder selected. Use the tree to browse children or create a node.</p>
    </article>
  );
}

function TextGroup({ node, mode, canClose, onClose, onSetMode, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & { node: RestNode; mode: "preview" | "edit"; canClose: boolean; onClose: () => void; onSetMode: (mode: "preview" | "edit") => void }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const setSaveState = useUiStore((state) => state.setSaveState);
  const textQuery = useQuery({ queryKey: queryKeys.text(node.space_id, node.id), queryFn: () => readText(client, node.space_id, node.id) });
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
  const saveMutation = useMutation({
    mutationFn: (force: boolean) => replaceText(client, node.space_id, node.id, draft, force ? undefined : sha),
    onMutate: () => setSaveState("saving"),
    onSuccess: () => {
      setSaveState("saved");
      setConflict(false);
      showToast("Saved");
      onSetMode("preview");
      void queryClient.invalidateQueries({ queryKey: queryKeys.text(node.space_id, node.id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.recent(node.space_id) });
      void queryClient.invalidateQueries({ queryKey: ["spaces", node.space_id] });
    },
    onError: (error) => {
      if (error instanceof ApiError && error.status === 409) {
        setConflict(true);
        setSaveState("conflict");
      } else {
        setSaveState("error");
      }
    }
  });
  const actions = (
    <>
      {mode === "edit" ? <Button onClick={() => saveMutation.mutate(false)} disabled={saveMutation.isPending || !dirty}>Save</Button> : null}
      <Button secondary onClick={() => onSetMode(mode === "edit" ? "preview" : "edit")} disabled={encrypted}>{mode === "edit" ? "Preview" : "Edit"}</Button>
      <NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null} />
    </>
  );
  return (
    <>
      <GroupHeader title={node.name} icon={<FileText size={17} />} actions={actions} canClose={canClose} onClose={onClose} dirty={dirty} />
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
                <button className="rounded-lg border border-border bg-surface px-3 py-1 text-xs text-text hover:bg-panel" onClick={() => { void textQuery.refetch(); setConflict(false); setSaveState("idle"); }}>Reload</button>
                <button className="rounded-lg border border-warning/50 px-3 py-1 text-xs text-warning hover:bg-warning/20" onClick={() => saveMutation.mutate(true)}>Overwrite</button>
              </div>
            </div>
          ) : null}
          {mode === "edit" ? (
            <textarea className="min-h-0 flex-1 resize-none bg-bg p-8 font-mono text-sm text-text outline-none" value={draft} onChange={(event) => setDraft(event.target.value)} />
          ) : isMarkdown ? (
            <div className="mx-auto w-full max-w-3xl overflow-y-auto px-10 py-14"><Markdown content={content} /></div>
          ) : (
            <article className="mx-auto max-w-3xl overflow-y-auto whitespace-pre-wrap px-10 py-14 font-mono text-sm text-text">{content}</article>
          )}
        </div>
      )}
    </>
  );
}

function FileView({ node }: { node: RestNode }) {
  const client = useApiClient();
  async function handleDownload() {
    const blob = await downloadFile(client, node.space_id, node.id);
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = node.original_filename ?? node.name;
    anchor.click();
    URL.revokeObjectURL(url);
  }
  return <article className="mx-auto max-w-3xl px-10 py-14"><p className="text-sm text-muted">{node.path}</p><h1 className="mt-4 text-4xl font-semibold tracking-tight">{node.name}</h1><dl className="mt-8 grid grid-cols-[120px_1fr] gap-y-3 text-sm"><dt className="font-semibold">Media type</dt><dd className="text-muted">{node.media_type ?? "unknown"}</dd><dt className="font-semibold">Bytes</dt><dd className="text-muted">{node.byte_len ?? 0}</dd><dt className="font-semibold">SHA-256</dt><dd className="break-all text-muted">{node.content_sha256}</dd></dl><button className="mt-8 inline-flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-semibold text-primary-contrast shadow-[var(--ng-inset-shadow)]" onClick={handleDownload}><Download size={16} /> Download</button></article>;
}
