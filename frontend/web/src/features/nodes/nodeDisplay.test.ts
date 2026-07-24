import { Database, FileText, Folder, Image as ImageIcon } from "lucide-react";
import { describe, expect, it } from "vitest";

import type { RestNode } from "../../entities/node/model";
import { nodeIcon } from "./nodeDisplay";

describe("nodeIcon", () => {
  it("uses an image icon only for server-verified previewable files", () => {
    expect(nodeIcon(node({ kind: "folder" }))).toBe(Folder);
    expect(nodeIcon(node({ kind: "text" }))).toBe(FileText);
    expect(nodeIcon(node({
      kind: "file",
      detected_media_type: "image/png",
      preview_available: true
    }))).toBe(ImageIcon);
    expect(nodeIcon(node({
      kind: "file",
      detected_media_type: "application/pdf",
      preview_available: true
    }))).toBe(FileText);
    expect(nodeIcon(node({ kind: "file", preview_available: false }))).toBe(Database);
    expect(nodeIcon(node({ kind: "file", preview_available: undefined }))).toBe(Database);
  });
});

function node(overrides: Partial<RestNode>): RestNode {
  return {
    id: "node-1",
    space_id: "space-1",
    parent_id: "root-1",
    name: "node",
    kind: "file",
    path: "/node",
    sort_order: 0,
    metadata: {},
    has_children: false,
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-22T00:00:00Z",
    updated_at: "2026-07-22T00:00:00Z",
    ...overrides
  };
}
