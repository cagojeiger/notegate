export const MAX_EDITOR_GROUPS = 3;

export const WORKBENCH_LAYOUT = {
  defaultPrimaryWidth: 300,
  minPrimaryWidth: 220,
  maxPrimaryWidth: 520,
  defaultAuxiliaryWidth: 320,
  minAuxiliaryWidth: 280,
  maxAuxiliaryWidth: 520,
  mobilePrimaryWidthPercent: "85%",
  mobilePrimaryMaxWidth: 320,
  defaultTreeRatio: 0.67,
  minTreeRatio: 0.2,
  maxTreeRatio: 0.82,
  defaultLinkRatio: 0.5,
  minLinkRatio: 0.2,
  maxLinkRatio: 0.8
} as const;

export type WorkbenchPanelMode = "hidden" | "overlay" | "docked";
export type EditorPresentation = "split" | "focused";
