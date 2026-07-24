import type { MouseEvent } from "react";

import type { RestNode } from "../../entities/node/model";

export type NodeContextPoint = Pick<MouseEvent, "clientX" | "clientY" | "preventDefault">;
export type NodeContextHandler = (node: RestNode, event: NodeContextPoint) => void;

export type TreeKeyboardNavigation = {
  focusLastNode: () => boolean;
};

export type TreeKeyboardNavigationRegistrar = (navigation: TreeKeyboardNavigation | null) => void;
