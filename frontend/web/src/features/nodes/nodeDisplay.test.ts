import { File, FileAudio, FileBadge2, FileText, Folder, Image as ImageIcon } from "lucide-react";
import { describe, expect, it } from "vitest";

import type { RestNode } from "../../api/types";
import { makeRestNode } from "../../test/fixtures";
import { nodeIcon } from "./nodeDisplay";

describe("nodeIcon", () => {
  it("uses an image icon only for server-verified previewable files", () => {
    expect(nodeIcon(node({ kind: "folder" }))).toBe(Folder);
    expect(nodeIcon(node({ kind: "text" }))).toBe(FileText);
    expect(nodeIcon(node({ kind: "file", file_preview_kind: "image" }))).toBe(ImageIcon);
    expect(nodeIcon(node({ kind: "file", file_preview_kind: "pdf" }))).toBe(FileBadge2);
    expect(nodeIcon(node({ kind: "file", file_media_kind: "audio" }))).toBe(FileAudio);
    expect(nodeIcon(node({ kind: "file", preview_available: true }))).toBe(ImageIcon);
    expect(nodeIcon(node({ kind: "file", preview_available: false }))).toBe(File);
    expect(nodeIcon(node({ kind: "file", preview_available: undefined }))).toBe(File);
  });
});

function node(overrides: Partial<RestNode>): RestNode {
  return makeRestNode({
    name: "node",
    kind: "file",
    path: "/node",
    ...overrides
  });
}
