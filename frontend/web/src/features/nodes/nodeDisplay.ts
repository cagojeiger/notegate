import type { RestNode, Space } from "../../api/types";

export function fmtBytes(bytes = 0): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

export function nodeMetaSuffix(node: RestNode): string | undefined {
  if (node.kind === "text" && node.line_count !== undefined) return `${node.line_count}l`;
  if (node.kind === "file" && node.byte_len !== undefined) return fmtBytes(node.byte_len);
  return undefined;
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
    has_children: true,
    created_by: { id: "", kind: "user", display_name: "" },
    updated_by: { id: "", kind: "user", display_name: "" },
    created_at: space.created_at,
    updated_at: space.updated_at
  };
}
