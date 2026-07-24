import type { RestNode, Space } from "../../api/types";
import type { AppDialog } from "./DialogHost";

export function newSpaceDialog(onCreate: (name: string) => void): AppDialog {
  return { kind: "prompt", title: "New space", label: "Space name", initial: "", submitLabel: "Create", onSubmit: onCreate };
}

export function renameSpaceDialog(space: Space, onRename: (spaceId: string, name: string) => void): AppDialog {
  return {
    kind: "prompt",
    title: "Rename space",
    label: "Space name",
    initial: space.name,
    submitLabel: "Rename",
    onSubmit: (name) => {
      if (name !== space.name) onRename(space.id, name);
    }
  };
}

export function deleteSpaceDialog(space: Space, onDelete: (spaceId: string) => void): AppDialog {
  return {
    kind: "confirm",
    title: "Delete space",
    message: `Delete space "${space.name}"? This permanently removes all of its nodes.`,
    danger: true,
    confirmLabel: "Delete",
    onConfirm: () => onDelete(space.id)
  };
}

export function createNodeDialog(parentId: string, kind: "folder" | "text", onCreate: (input: { parentId: string; kind: "folder" | "text"; name: string; content?: string }) => void): AppDialog {
  return {
    kind: "prompt",
    title: kind === "folder" ? "New folder" : "New text",
    label: "Name",
    initial: "",
    submitLabel: "Create",
    onSubmit: (name) => onCreate({ parentId, kind, name, content: kind === "text" ? "" : undefined })
  };
}

export function uploadFileDialog(parentId: string, file: File, onUpload: (input: { parentId: string; name: string; file: File }) => void): AppDialog {
  return {
    kind: "prompt",
    title: "Upload file",
    label: "File node name",
    initial: file.name,
    submitLabel: "Upload",
    onSubmit: (name) => onUpload({ parentId, name, file })
  };
}

export function renameNodeDialog(node: RestNode, onRename: (node: RestNode, name: string) => void): AppDialog {
  return {
    kind: "prompt",
    title: "Rename",
    label: "Name",
    initial: node.name,
    submitLabel: "Rename",
    onSubmit: (name) => {
      if (name !== node.name) onRename(node, name);
    }
  };
}

export function moveNodeDialog(node: RestNode, space: Space, onMove: (node: RestNode, parentId: string) => void): AppDialog {
  return { kind: "move", node, space, onMove: (parentId) => onMove(node, parentId) };
}

export function deleteNodeDialog(node: RestNode, onDelete: (node: RestNode, recursive: boolean) => void): AppDialog {
  const recursive = node.kind === "folder";
  return {
    kind: "confirm",
    title: "Delete",
    message: `Delete "${node.name}"${recursive ? " and everything inside it" : ""}?`,
    danger: true,
    confirmLabel: "Delete",
    onConfirm: () => onDelete(node, recursive)
  };
}

export function metadataDialog(node: RestNode, onSave: (node: RestNode, metadata: Record<string, unknown>) => void): AppDialog {
  return { kind: "metadata", node, onSave: (metadata) => onSave(node, metadata) };
}
