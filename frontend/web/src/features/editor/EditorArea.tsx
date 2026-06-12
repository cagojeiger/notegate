import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database, Download, FileText, Folder, MoreHorizontal, Trash2, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { downloadFile } from "../../api/files";
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
  onCreateFolder: () => void;
  onCreateText: () => void;
  onFileSelected: (file: File | null) => void;
};

export function EditorArea({ groups, activeGroupIndex, activeSpace, onFocusGroup, onCloseGroup, onCreateFolder, onCreateText, onFileSelected, onRenameNode, onMoveNode, onDeleteNode }: EditorAreaProps) {
  const multiple = groups.length > 1;
  return (
    <div className="flex min-w-0 flex-1">
      {groups.map((group, index) => {
        const active = index === activeGroupIndex;
        return (
          <section
            key={group.id}
            onMouseDown={() => onFocusGroup(index)}
            className={`flex min-w-0 flex-1 flex-col bg-bg ${index > 0 ? "border-l border-border" : ""} ${active ? "" : "max-md:hidden"} ${multiple && active ? "ring-1 ring-inset ring-primary/40" : ""}`}
          >
            <GroupBody
              node={group.node}
              activeSpace={activeSpace}
              canClose={multiple}
              onClose={() => onCloseGroup(index)}
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

function GroupBody({ node, activeSpace, canClose, onClose, onCreateFolder, onCreateText, onFileSelected, onRenameNode, onMoveNode, onDeleteNode }: NodeActions & { node: RestNode | null; activeSpace: Space | null; canClose: boolean; onClose: () => void; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void }) {
  if (!node) {
    return (
      <>
        <GroupHeader title="Open a node" canClose={canClose} onClose={onClose} />
        <EmptyEditor activeSpace={activeSpace} onCreateFolder={onCreateFolder} onCreateText={onCreateText} onFileSelected={onFileSelected} />
      </>
    );
  }
  const Icon = node.kind === "folder" ? Folder : node.kind === "file" ? Database : FileText;
  return (
    <>
      <GroupHeader
        title={node.name}
        icon={<Icon size={17} />}
        canClose={canClose}
        onClose={onClose}
        actions={<NodeActionMenu onRenameNode={() => onRenameNode(node)} onMoveNode={() => onMoveNode(node)} onDeleteNode={() => onDeleteNode(node)} disabled={node.parent_id === null} />}
      />
      {node.kind === "text" ? <TextEditor node={node} /> : node.kind === "file" ? <FileView node={node} /> : <FolderView node={node} />}
    </>
  );
}

function GroupHeader({ title, icon, actions, canClose, onClose }: { title: string; icon?: ReactNode; actions?: ReactNode; canClose: boolean; onClose: () => void }) {
  return (
    <div className="flex h-12 items-center justify-between border-b border-border px-4">
      <div className="flex min-w-0 items-center gap-2 font-semibold">{icon}<span className="truncate">{title}</span></div>
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
  return (
    <div className="relative">
      <IconButton label="Node actions" onClick={() => setOpen((value) => !value)} disabled={disabled}><MoreHorizontal size={16} /></IconButton>
      {open ? (
        <div className="absolute right-0 top-9 z-20 w-40 rounded-xl border border-border bg-surface p-1 text-sm shadow-[var(--ng-focus-shadow)]">
          <MenuButton onClick={() => { onRenameNode(); setOpen(false); }}>Rename</MenuButton>
          <MenuButton onClick={() => { onMoveNode(); setOpen(false); }}>Move</MenuButton>
          <MenuButton danger onClick={() => { onDeleteNode(); setOpen(false); }}><Trash2 size={14} /> Delete</MenuButton>
        </div>
      ) : null}
    </div>
  );
}

function FolderView({ node }: { node: RestNode }) {
  return <article className="mx-auto max-w-3xl px-10 py-14"><p className="text-sm text-muted">{node.path}</p><h1 className="mt-4 text-4xl font-semibold tracking-tight">{node.name}</h1><p className="mt-8 leading-7 text-muted">Folder selected. Use the tree to browse children or create a node.</p></article>;
}

function TextEditor({ node }: { node: RestNode }) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const showToast = useUiStore((state) => state.showToast);
  const textQuery = useQuery({ queryKey: queryKeys.text(node.space_id, node.id), queryFn: () => readText(client, node.space_id, node.id) });
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const text = textQuery.data?.text;
  const content = text && "content" in text ? text.content : "";
  const sha = text && "content_sha256" in text ? text.content_sha256 : undefined;
  useEffect(() => {
    setEditing(false);
    setDraft("");
  }, [node.id]);
  const saveMutation = useMutation({
    mutationFn: () => replaceText(client, node.space_id, node.id, draft, sha),
    onSuccess: () => {
      setEditing(false);
      showToast("Saved");
      void queryClient.invalidateQueries({ queryKey: queryKeys.text(node.space_id, node.id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.recent(node.space_id) });
      void queryClient.invalidateQueries({ queryKey: ["spaces", node.space_id] });
    }
  });
  if (textQuery.isLoading) return <div className="p-10 text-muted">Loading text…</div>;
  if (textQuery.isError) return <div className="p-10 text-danger">Could not load text.</div>;
  if (text && "encrypted_payload" in text) return <div className="p-10 text-muted">Encrypted text cannot be previewed by the server.</div>;
  const isMarkdown = /\.(md|markdown)$/i.test(node.name);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex justify-end border-b border-border px-4 py-2">
        {editing ? <Button onClick={() => saveMutation.mutate()} disabled={saveMutation.isPending}>Save</Button> : <Button secondary onClick={() => { setDraft(content); setEditing(true); }}>Edit</Button>}
      </div>
      {editing ? (
        <textarea className="min-h-0 flex-1 resize-none bg-bg p-8 font-mono text-sm text-text outline-none" value={draft} onChange={(event) => setDraft(event.target.value)} />
      ) : isMarkdown ? (
        <div className="mx-auto w-full max-w-3xl overflow-y-auto px-10 py-14"><Markdown content={content} /></div>
      ) : (
        <article className="mx-auto max-w-3xl overflow-y-auto whitespace-pre-wrap px-10 py-14 font-mono text-sm text-text">{content}</article>
      )}
    </div>
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
