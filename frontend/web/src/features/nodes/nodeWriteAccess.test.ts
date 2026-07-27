import { describe, expect, it } from "vitest";

import type { NodeSummary, RestNode } from "../../api/types";
import {
  canCreateInFolder,
  canMoveNodeToFolder,
  canMutateNode,
  canWriteNode,
  resolveNodeCreateTarget
} from "./nodeWriteAccess";

const node: NodeSummary = {
  id: "text-1",
  space_id: "space-1",
  parent_id: "folder-1",
  name: "note.md",
  kind: "text",
  path: "/Policies/note.md",
  has_children: false,
  effective_write_locked: false,
  updated_at: "2026-07-27T00:00:00Z"
};

const folder: NodeSummary = {
  ...node,
  id: "folder-1",
  parent_id: "root-1",
  name: "Policies",
  kind: "folder",
  path: "/Policies",
  has_children: true
};

describe("node write access", () => {
  it("requires both space write access and an unlocked node", () => {
    expect(canWriteNode(node, true)).toBe(true);
    expect(canWriteNode(node, false)).toBe(false);
    expect(canWriteNode({ ...node, effective_write_locked: true }, true)).toBe(false);
  });

  it("keeps root and locked nodes out of direct mutations", () => {
    expect(canMutateNode(node, true)).toBe(true);
    expect(canMutateNode({ ...node, parent_id: null }, true)).toBe(false);
    expect(canMutateNode({ ...node, effective_write_locked: true }, true)).toBe(false);
  });

  it("allows creation only in writable folders", () => {
    expect(canCreateInFolder(folder, true)).toBe(true);
    expect(canCreateInFolder(node, true)).toBe(false);
    expect(canCreateInFolder({ ...folder, effective_write_locked: true }, true)).toBe(false);
  });

  it("requires a writable source and destination for moves", () => {
    expect(canMoveNodeToFolder(node, folder, true)).toBe(true);
    expect(canMoveNodeToFolder({ ...node, effective_write_locked: true }, folder, true)).toBe(false);
    expect(canMoveNodeToFolder(node, { ...folder, effective_write_locked: true }, true)).toBe(false);
    expect(canMoveNodeToFolder(folder, folder, true)).toBe(false);
  });

  it("uses the active folder or space root as the create target", () => {
    expect(resolveNodeCreateTarget("root-1", null)).toEqual({
      id: "root-1",
      path: "/",
      writeLocked: false
    });
    expect(resolveNodeCreateTarget("root-1", restNode(folder))).toEqual({
      id: "folder-1",
      path: "/Policies",
      writeLocked: false
    });
  });

  it("separates a document's direct lock from inherited parent locks", () => {
    expect(resolveNodeCreateTarget("root-1", restNode(node, {
      write_locked: true,
      effective_write_locked: true,
      write_lock_sources: [
        { node_id: node.id, name: node.name, path: node.path }
      ]
    }))).toEqual({
      id: "folder-1",
      path: "/Policies",
      writeLocked: false
    });

    expect(resolveNodeCreateTarget("root-1", restNode(node, {
      effective_write_locked: true,
      write_lock_sources: [
        { node_id: folder.id, name: folder.name, path: folder.path }
      ]
    }))).toEqual({
      id: "folder-1",
      path: "/Policies",
      writeLocked: true
    });
  });
});

function restNode(
  summary: NodeSummary,
  overrides: Partial<RestNode> = {}
): RestNode {
  return {
    ...summary,
    sort_order: 0,
    metadata: {},
    search_enabled: true,
    write_locked: false,
    write_lock_sources: [],
    created_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    created_at: "2026-07-27T00:00:00Z",
    ...overrides
  };
}
