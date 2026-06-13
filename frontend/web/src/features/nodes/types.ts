import type { MouseEvent } from "react";

import type { RestNode } from "../../api/types";

export type NodeContextHandler = (node: RestNode, event: MouseEvent) => void;
