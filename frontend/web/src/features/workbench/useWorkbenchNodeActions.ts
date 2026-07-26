import type { Dispatch, SetStateAction } from "react";

import type { RestNode, Space } from "../../api/types";
import type { AppDialog } from "./dialogs/dialogTypes";
import { useCanonicalNodeLoader } from "./useCanonicalNodeLoader";
import { useWorkbenchNodeCommandActions } from "./useWorkbenchNodeCommandActions";
import { useWorkbenchNodeNavigationActions } from "./useWorkbenchNodeNavigationActions";

type NodeActionsProps = {
  activeSpace: Space | null;
  activeNode: RestNode | null;
  canWriteActiveSpace: boolean;
  canManageActiveSpace: boolean;
  setDialog: Dispatch<SetStateAction<AppDialog | null>>;
};

export function useWorkbenchNodeActions(props: NodeActionsProps) {
  const loadCanonicalNode = useCanonicalNodeLoader();
  const navigationActions = useWorkbenchNodeNavigationActions({
    activeSpace: props.activeSpace,
    loadCanonicalNode
  });
  const commandActions = useWorkbenchNodeCommandActions({
    ...props,
    loadCanonicalNode
  });

  return {
    ...navigationActions,
    ...commandActions
  };
}
