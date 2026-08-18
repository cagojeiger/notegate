import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { ApiError } from "../../api/errors";
import { getNode, resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { NodeRevealResponse, NodeSummary, RestNode, Space } from "../../api/types";
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
    await openNodeFromSummary(summary, openInActiveGroup);
  }

  async function openNodeInNewGroup(summary: NodeSummary) {
    await openNodeFromSummary(summary, openInNewGroup);
  }

  async function openNodeById(nodeId: string) {
    if (!activeSpace) return;
    const spaceId = activeSpace.id;
    let resolved: { node: RestNode; reveal: NodeRevealResponse | null };
    try {
      resolved = await loadNavigationNode(spaceId, nodeId);
    } catch (error) {
      showToast(error instanceof ApiError && error.status === 404 ? "Link target not found" : "Could not open link target");
      return;
    }
    if (useUiStore.getState().activeSpaceId !== spaceId || resolved.node.space_id !== spaceId) return;

    if (resolved.reveal) applyReveal(resolved.reveal);
    openInActiveGroup(resolved.node);
    closeMobile();
    if (!resolved.reveal) showToast("Opened item, but could not reveal it in Files");
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
      showToast(error instanceof ApiError && error.status === 404 ? "Link target not found" : "Could not open link target");
      return;
    }

    if (node.space_id !== spaceId) {
      showToast("Could not open link target");
      return;
    }
    if (!isCurrentMarkdownLinkSource(spaceId, groupId, sourceNode)) return;

    cacheCanonicalNode(node);
    openInGroup(groupId, node);
    closeMobile();
    await revealNodeBestEffort(spaceId, node);
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

        let resolved: {
          node: RestNode;
          reveal: NodeRevealResponse | null;
        };
        try {
          resolved = await loadNavigationNode(spaceId, target.nodeId);
        } catch (error) {
          if (error instanceof ApiError && error.status === 404) {
            if (!discardNavigationTarget(groupId, direction, target.nodeId)) return;
            continue;
          }
          showToast(`Could not go ${direction}`);
          return;
        }

        if (
          resolved.node.space_id !== spaceId
          || !isCurrentNavigationTarget(spaceId, groupId, direction, target.nodeId)
        ) return;
        if (resolved.reveal) applyReveal(resolved.reveal);
        if (!navigateGroup(groupId, direction, target.nodeId, resolved.node)) return;
        closeMobile();
        if (!resolved.reveal) {
          showToast("Opened item, but could not reveal it in Files");
        }
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

  function isCurrentNavigationTarget(
    spaceId: string,
    groupId: number,
    direction: EditorNavigationDirection,
    nodeId: string
  ): boolean {
    const state = useUiStore.getState();
    const group = state.editorGroups.find((candidate) => candidate.id === groupId);
    const entries = group?.[direction] ?? [];
    return state.activeSpaceId === spaceId && entries[entries.length - 1]?.nodeId === nodeId;
  }

  async function openNodeFromSummary(
    summary: NodeSummary,
    open: (node: RestNode) => void
  ) {
    let revealFailed = false;
    if (activeSpace?.id === summary.space_id && summary.parent_id !== null) {
      let reveal: NodeRevealResponse | null = null;
      try {
        reveal = await requestReveal(activeSpace.id, summary.id);
      } catch {
        revealFailed = true;
      }
      if (reveal) {
        applyReveal(reveal);
        open(reveal.target);
        closeMobile();
        return;
      }
    }

    const node = await loadCanonicalNode(summary, "Could not open item");
    if (!node) return;
    open(node);
    closeMobile();
    if (revealFailed) {
      showToast("Opened item, but could not reveal it in Files");
    }
  }

  async function revealNodeBestEffort(spaceId: string, node: NodeSummary) {
    if (node.parent_id === null) return;
    try {
      applyReveal(await requestReveal(spaceId, node.id));
    } catch {
      showToast("Opened item, but could not reveal it in Files");
    }
  }

  async function requestReveal(spaceId: string, nodeId: string): Promise<NodeRevealResponse> {
    let reveal: NodeRevealResponse | null = null;
    // A row click selects the Inspector before opening the editor. Use the
    // canonical key so that observer shares this in-flight reveal request.
    await queryClient.fetchQuery({
      queryKey: queryKeys.node(spaceId, nodeId),
      queryFn: async () => {
        reveal = await fetchReveal(spaceId, nodeId);
        return reveal.target;
      },
      staleTime: 0,
      retry: false
    });
    if (reveal) return reveal;
    // Another canonical request already owned the key, so fetch the ancestor
    // context that its RestNode result does not contain.
    return fetchReveal(spaceId, nodeId);
  }

  async function fetchReveal(spaceId: string, nodeId: string): Promise<NodeRevealResponse> {
    const reveal = await revealNodeInSpace(spaceId, nodeId);
    if (reveal.target.space_id !== spaceId || reveal.target.id !== nodeId) {
      throw new Error("Reveal returned a different node");
    }
    return reveal;
  }

  async function loadNavigationNode(
    spaceId: string,
    nodeId: string
  ): Promise<{ node: RestNode; reveal: NodeRevealResponse | null }> {
    try {
      const reveal = await requestReveal(spaceId, nodeId);
      return { node: reveal.target, reveal };
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) throw error;
      const node = await queryClient.fetchQuery({
        queryKey: queryKeys.node(spaceId, nodeId),
        queryFn: () => getNode(client, spaceId, nodeId),
        staleTime: 0
      });
      return { node, reveal: null };
    }
  }

  function applyReveal(reveal: NodeRevealResponse) {
    addExpanded(reveal.ancestors.map((ancestor) => ancestor.id));
  }

  function cacheCanonicalNode(node: RestNode) {
    queryClient.setQueryData(queryKeys.node(node.space_id, node.id), node);
  }

  return {
    openNode,
    openNodeInNewGroup,
    openNodeById,
    openMarkdownLink,
    navigateEditorGroup,
    navigatingGroupIds
  };
}
