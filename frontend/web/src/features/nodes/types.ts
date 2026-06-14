import type { MouseEvent } from "react";

import type { RestNode } from "../../api/types";

export type NodeContextPoint = Pick<MouseEvent, "clientX" | "clientY" | "preventDefault">;
export type NodeContextHandler = (node: RestNode, event: NodeContextPoint) => void;
