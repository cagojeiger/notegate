import type { MouseEvent } from "react";

import type { NodeSummary } from "../../api/types";

export type NodeContextPoint = Pick<MouseEvent, "clientX" | "clientY" | "preventDefault">;
export type NodeContextHandler = (node: NodeSummary, event: NodeContextPoint) => void;

export type TreeKeyboardNavigation = {
  focusLastNode: () => boolean;
};

export type TreeKeyboardNavigationRegistrar = (navigation: TreeKeyboardNavigation | null) => void;
