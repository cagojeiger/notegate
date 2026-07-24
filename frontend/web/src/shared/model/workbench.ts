export const MAX_EDITOR_GROUPS = 3;

export const WORKBENCH_LAYOUT = {
  defaultPrimaryWidth: 300,
  minPrimaryWidth: 220,
  maxPrimaryWidth: 520,
  auxiliaryWidth: 320,
  mobilePrimaryWidthPercent: "85%",
  mobilePrimaryMaxWidth: 320,
  defaultTreeRatio: 0.67,
  minTreeRatio: 0.2,
  maxTreeRatio: 0.82
} as const;

export type WorkbenchPanelMode = "hidden" | "overlay" | "docked";
export type EditorPresentation = "split" | "focused";
export type SaveState = "idle" | "saving" | "saved" | "error" | "conflict";

export type OpenedNodeRef = {
  nodeId: string;
  spaceId: string;
};

// Server node data stays in React Query. A group only owns the identity of the
// node it shows and its preview/edit mode.
export type EditorGroup = {
  id: number;
  nodeRef: OpenedNodeRef | null;
  mode: "preview" | "edit";
};

export type EditorGroupState = {
  editorGroups: EditorGroup[];
  activeGroupIndex: number;
  nextGroupId: number;
};
