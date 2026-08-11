import { File, FileAudio, FileBadge2, FileText, Folder, Image as ImageIcon, type LucideIcon } from "lucide-react";

import type { NodeSummary, RestNode, Space } from "../../api/types";

export function nodeIcon(node: NodeSummary): LucideIcon {
  if (node.kind === "folder") return Folder;
  if (node.kind === "text") return FileText;
  if (node.file_media_kind === "audio") return FileAudio;
  if (node.file_preview_kind === "pdf") return FileBadge2;
  if (node.file_preview_kind === "image" || node.preview_available === true) return ImageIcon;
  return File;
}

export function makeRootNode(space: Space): RestNode {
  return {
    id: space.root_node_id,
    space_id: space.id,
    parent_id: null,
    name: "/",
    kind: "folder",
    path: "/",
    sort_order: 0,
    metadata: {},
    search_enabled: true,
    write_locked: false,
    effective_write_locked: false,
    write_lock_sources: [],
    has_children: true,
    created_by: { id: "", kind: "user", display_name: "" },
    updated_by: { id: "", kind: "user", display_name: "" },
    created_at: space.created_at,
    updated_at: space.updated_at
  };
}
