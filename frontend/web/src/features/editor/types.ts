import type { RestNode } from "../../api/types";

export type NodeActions = {
  onRenameNode: (node: RestNode) => void;
  onMoveNode: (node: RestNode) => void;
  onDeleteNode: (node: RestNode) => void;
};

export type EditorNavigationActions = {
  onOpenNodeInNewGroup: (node: RestNode) => void;
  onOpenMarkdownLink: (groupId: number, node: RestNode, path: string) => void;
};
