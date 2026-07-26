import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { getNode, resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { NodeSummary, RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import type { EditorNavigationDirection } from "../../stores/uiStoreReducers";
import type { CanonicalNodeLoader } from "./useCanonicalNodeLoader";
import { useRevealNode } from "./useWorkbenchQueries";

type NavigationActionsProps = {
  activeSpace: Space | null;
  loadCanonicalNode: CanonicalNodeLoader;
};

export function useWorkbenchNodeNavigationActions({
  activeSpace,
  loadCanonicalNode
}: NavigationActionsProps) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const openInActiveGroup = useUiStore((state) => state.openInActiveGroup);
  const openInGroup = useUiStore((state) => state.openInGroup);
  const openInNewGroup = useUiStore((state) => state.openInNewGroup);
  const navigateGroup = useUiStore((state) => state.navigateGroup);
  const discardNavigationTarget = useUiStore((state) => state.discardNavigationTarget);
  const addExpanded = useUiStore((state) => state.addExpanded);
  const closeMobile = useUiStore((state) => state.closeMobile);
  const showToast = useUiStore((state) => state.showToast);
  const revealNodeInSpace = useRevealNode();
  const navigatingGroupsRef = useRef(new Set<number>());
  const [navigatingGroupIds, setNavigatingGroupIds] = useState<ReadonlySet<number>>(new Set());

  async function openNode(summary: NodeSummary) {
    const node = await loadCanonicalNode(summary, "Could not open node");
    if (!node) return;
    openInActiveGroup(node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function openNodeInNewGroup(summary: NodeSummary) {
    const node = await loadCanonicalNode(summary, "Could not open node");
    if (!node) return;
    openInNewGroup(node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function openMarkdownLink(groupId: number, sourceNode: RestNode, path: string) {
    if (
      !activeSpace
      || sourceNode.space_id !== activeSpace.id
      || !isCurrentMarkdownLinkSource(activeSpace.id, groupId, sourceNode)
    ) return;
    const spaceId = activeSpace.id;

    let node: RestNode;
    try {
      node = await resolveNodePath(client, spaceId, path);
    } catch (error) {
      showToast(error instanceof ApiError && error.status === 404 ? "Linked node not found" : "Could not open linked node");
      return;
    }

    if (node.space_id !== spaceId) {
      showToast("Could not open linked node");
      return;
    }
    if (!isCurrentMarkdownLinkSource(spaceId, groupId, sourceNode)) return;

    openInGroup(groupId, node);
    closeMobile();
    await revealNodeBestEffort(node);
  }

  async function navigateEditorGroup(groupId: number, direction: EditorNavigationDirection) {
    if (!activeSpace || navigatingGroupsRef.current.has(groupId)) return;
    const spaceId = activeSpace.id;
    setGroupNavigationPending(groupId, true);

    try {
      while (true) {
        const state = useUiStore.getState();
        if (state.activeSpaceId !== spaceId) return;
        const group = state.editorGroups.find((candidate) => candidate.id === groupId);
        const entries = group?.[direction] ?? [];
        const target = entries[entries.length - 1];
        if (!target || target.spaceId !== spaceId) return;

        let node: RestNode;
        try {
          node = await queryClient.fetchQuery({
            queryKey: queryKeys.node(spaceId, target.nodeId),
            queryFn: () => getNode(client, spaceId, target.nodeId),
            staleTime: 0
          });
        } catch (error) {
          if (error instanceof ApiError && error.status === 404) {
            if (!discardNavigationTarget(groupId, direction, target.nodeId)) return;
            continue;
          }
          showToast("Could not navigate to node");
          return;
        }

        if (node.space_id !== spaceId || !navigateGroup(groupId, direction, target.nodeId, node)) return;
        closeMobile();
        await revealNodeBestEffort(node);
        return;
      }
    } finally {
      setGroupNavigationPending(groupId, false);
    }
  }

  function setGroupNavigationPending(groupId: number, pending: boolean) {
    const next = new Set(navigatingGroupsRef.current);
    if (pending) next.add(groupId);
    else next.delete(groupId);
    navigatingGroupsRef.current = next;
    setNavigatingGroupIds(next);
  }

  function isCurrentMarkdownLinkSource(spaceId: string, groupId: number, sourceNode: RestNode): boolean {
    const state = useUiStore.getState();
    return state.activeSpaceId === spaceId
      && state.editorGroups.some((group) => group.id === groupId && group.node?.id === sourceNode.id);
  }

  async function revealNodeBestEffort(node: NodeSummary) {
    try {
      await revealNode(node);
    } catch {
      showToast("Opened node, but could not reveal it in the tree");
    }
  }

  async function revealNode(node: NodeSummary) {
    if (!activeSpace || node.parent_id === null) return;
    const reveal = await revealNodeInSpace(activeSpace.id, node.id);
    addExpanded(reveal.ancestors.map((ancestor) => ancestor.id));
  }

  return {
    openNode,
    openNodeInNewGroup,
    openMarkdownLink,
    navigateEditorGroup,
    navigatingGroupIds
  };
}
