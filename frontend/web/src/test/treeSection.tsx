import type { ComponentProps } from "react";
import { vi } from "vitest";

import type { RestNode, Space } from "../api/types";
import { TreeSection } from "../features/nodes/TreeSection";
import { makeRestNode } from "./fixtures";

export function treeSectionElement(activeSpace: Space, overrides: Partial<ComponentProps<typeof TreeSection>> = {}) {
  return (
    <TreeSection
      activeSpace={activeSpace}
      openedNodeId={null}
      inspectedNodeId={null}
      expandedFolderIds={new Set()}
      open
      onToggle={vi.fn()}
      headerActions={null}
      onToggleFolder={vi.fn()}
      onInspectNode={vi.fn()}
      onOpenNode={vi.fn()}
      onNodeContextMenu={vi.fn()}
      onMoveNodeToFolder={vi.fn()}
      onTreeNavigationChange={vi.fn()}
      canWriteActiveSpace
      {...overrides}
    />
  );
}

export function createTreeNodeFactory(space: Space) {
  return function node(id: string, kind: RestNode["kind"], parentId = space.root_node_id, path?: string): RestNode {
    const name = kind === "folder" ? id : `${id}.${kind === "text" ? "md" : "bin"}`;
    return makeRestNode({
      id,
      space_id: space.id,
      parent_id: parentId,
      name,
      kind,
      path: path ?? `/${name}`,
      has_children: kind === "folder"
    });
  };
}
