import { useQuery } from "@tanstack/react-query";
import { ChevronRight, Folder, FolderOpen } from "lucide-react";
import { useMemo, useState } from "react";

import { useApiClient } from "../../api/ApiProvider";
import { listChildren } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode, Space } from "../../api/types";
import { Button, Modal } from "../../shared/ui";

// Discriminated union of every in-app dialog. Replaces window.prompt/confirm so
// flows match the app tone and stay keyboard/escape friendly.
export type AppDialog =
  | { kind: "prompt"; title: string; label: string; initial: string; placeholder?: string; submitLabel?: string; onSubmit: (value: string) => void }
  | { kind: "confirm"; title: string; message: string; danger?: boolean; confirmLabel?: string; onConfirm: () => void }
  | { kind: "move"; node: RestNode; space: Space; onMove: (parentId: string) => void }
  | { kind: "metadata"; node: RestNode; onSave: (metadata: Record<string, unknown>) => void };

export function DialogHost({ dialog, onClose }: { dialog: AppDialog | null; onClose: () => void }) {
  if (!dialog) return null;
  if (dialog.kind === "prompt") return <PromptDialog dialog={dialog} onClose={onClose} />;
  if (dialog.kind === "confirm") return <ConfirmDialog dialog={dialog} onClose={onClose} />;
  if (dialog.kind === "move") return <MoveDialog dialog={dialog} onClose={onClose} />;
  return <MetadataDialog dialog={dialog} onClose={onClose} />;
}

function PromptDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "prompt" }>; onClose: () => void }) {
  const [value, setValue] = useState(dialog.initial);
  const trimmed = value.trim();
  function submit() {
    if (!trimmed) return;
    dialog.onSubmit(trimmed);
    onClose();
  }
  return (
    <Modal
      title={dialog.title}
      onClose={onClose}
      footer={<><Button secondary onClick={onClose}>Cancel</Button><Button onClick={submit} disabled={!trimmed}>{dialog.submitLabel ?? "Save"}</Button></>}
    >
      <label className="block text-sm">
        <span className="mb-1.5 block text-xs font-semibold uppercase tracking-wide text-muted">{dialog.label}</span>
        <input
          autoFocus
          value={value}
          placeholder={dialog.placeholder}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Enter") submit(); }}
          className="w-full rounded-lg border border-border bg-surface px-3 py-2 text-text outline-none"
        />
      </label>
    </Modal>
  );
}

function ConfirmDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "confirm" }>; onClose: () => void }) {
  function confirm() {
    dialog.onConfirm();
    onClose();
  }
  return (
    <Modal
      title={dialog.title}
      onClose={onClose}
      footer={
        <>
          <Button secondary onClick={onClose}>Cancel</Button>
          <button
            type="button"
            onClick={confirm}
            className={dialog.danger
              ? "rounded-lg bg-danger px-3 py-2 text-sm font-semibold text-primary-contrast hover:opacity-90"
              : "rounded-lg bg-primary px-3 py-2 text-sm font-semibold text-primary-contrast hover:bg-[var(--ng-primary-hover)]"}
          >
            {dialog.confirmLabel ?? "Confirm"}
          </button>
        </>
      }
    >
      <p className="text-sm leading-6 text-muted">{dialog.message}</p>
    </Modal>
  );
}

type Crumb = { id: string; name: string };

function MoveDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "move" }>; onClose: () => void }) {
  const { node, space } = dialog;
  const client = useApiClient();
  const [stack, setStack] = useState<Crumb[]>([{ id: space.root_node_id, name: "/" }]);
  const current = stack[stack.length - 1];
  const childrenQuery = useQuery({
    queryKey: [...queryKeys.children(space.id, current.id), "move-picker"],
    queryFn: () => listChildren(client, space.id, current.id)
  });
  // Only folders are valid destinations; never let the user descend into the
  // node being moved (that would also block reaching its descendants).
  const folders = useMemo(
    () => (childrenQuery.data?.children ?? []).filter((child) => child.kind === "folder" && child.id !== node.id),
    [childrenQuery.data, node.id]
  );
  const alreadyHere = node.parent_id === current.id;
  return (
    <Modal
      title={`Move "${node.name}"`}
      onClose={onClose}
      footer={
        <>
          <Button secondary onClick={onClose}>Cancel</Button>
          <Button onClick={() => { dialog.onMove(current.id); onClose(); }} disabled={alreadyHere}>Move here</Button>
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-1 text-xs text-muted">
        {stack.map((crumb, index) => (
          <span key={crumb.id} className="flex items-center gap-1">
            {index > 0 ? <ChevronRight size={12} className="text-faint" /> : null}
            <button
              type="button"
              onClick={() => setStack((prev) => prev.slice(0, index + 1))}
              className={`rounded px-1 py-0.5 hover:bg-surface hover:text-text ${index === stack.length - 1 ? "font-semibold text-text" : ""}`}
            >
              {crumb.name === "/" ? "Root" : crumb.name}
            </button>
          </span>
        ))}
      </div>
      <div className="mt-3 max-h-64 min-h-[8rem] overflow-y-auto rounded-xl border border-border bg-surface p-1">
        {childrenQuery.isLoading ? (
          <div className="px-3 py-2 text-sm text-muted">Loading…</div>
        ) : folders.length === 0 ? (
          <div className="flex items-center gap-2 px-3 py-2 text-sm text-faint"><FolderOpen size={14} /> No subfolders here</div>
        ) : (
          folders.map((folder) => (
            <button
              key={folder.id}
              type="button"
              onClick={() => setStack((prev) => [...prev, { id: folder.id, name: folder.name }])}
              className="flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm text-muted hover:bg-panel hover:text-text"
            >
              <span className="flex min-w-0 items-center gap-2"><Folder size={14} className="shrink-0" /><span className="truncate">{folder.name}</span></span>
              <ChevronRight size={14} className="shrink-0 text-faint" />
            </button>
          ))
        )}
      </div>
      <p className="mt-3 text-xs text-muted">Destination: <span className="font-mono text-text">{current.name === "/" ? "/" : current.name}</span>{alreadyHere ? " (already here)" : ""}</p>
    </Modal>
  );
}

function MetadataDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "metadata" }>; onClose: () => void }) {
  const [text, setText] = useState(() => JSON.stringify(dialog.node.metadata ?? {}, null, 2));
  const parsed = useMemo<{ ok: true; value: Record<string, unknown> } | { ok: false; error: string }>(() => {
    try {
      const value = JSON.parse(text);
      if (typeof value !== "object" || value === null || Array.isArray(value)) return { ok: false, error: "Metadata must be a JSON object" };
      return { ok: true, value: value as Record<string, unknown> };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : "Invalid JSON" };
    }
  }, [text]);
  return (
    <Modal
      title="Edit metadata"
      onClose={onClose}
      width="max-w-lg"
      footer={<><Button secondary onClick={onClose}>Cancel</Button><Button onClick={() => { if (parsed.ok) { dialog.onSave(parsed.value); onClose(); } }} disabled={!parsed.ok}>Save</Button></>}
    >
      <textarea
        autoFocus
        value={text}
        onChange={(event) => setText(event.target.value)}
        spellCheck={false}
        rows={10}
        className="w-full resize-y rounded-lg border border-border bg-surface p-3 font-mono text-xs text-text outline-none"
      />
      <p className={`mt-2 text-xs ${parsed.ok ? "text-faint" : "text-danger"}`}>{parsed.ok ? "Valid JSON object. Metadata is stored unencrypted." : parsed.error}</p>
    </Modal>
  );
}
