import type { NodeSummary, RestNode } from "../../api/types";

export type NodeActions = {
  onRenameNode: (node: NodeSummary) => void;
  onMoveNode: (node: NodeSummary) => void;
  onDeleteNode: (node: NodeSummary) => void;
};

export type EditorNavigationActions = {
  onOpenNodeInNewGroup: (node: NodeSummary) => void;
  onOpenMarkdownLink: (groupId: number, node: RestNode, path: string) => void;
};
