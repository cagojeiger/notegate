import { useMutation, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { replaceMetadata } from "../../api/metadata";
import { updateNodeCaches } from "../../api/nodeCache";
import {
  createNode,
  deleteNode,
  moveNode,
  revealNode,
  updateNode,
  updateNodeSearchPolicy
} from "../../api/nodes";
import { updateTextEncryption } from "../../api/text";
import {
  invalidateFolderSubtree,
  invalidateNodeLists,
  invalidateRecentNodes,
  invalidateText,
  removeDeletedNodeQueries,
  removeMarkdownImagePreviewQuery
} from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { NodeSummary, RestNode, Space } from "../../api/types";

type MoveNodeMutationOptions = {
  silentError?: boolean;
};

export function useCreateNodeMutation(activeSpace: Space | null, onCreated: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: ({ parentId, kind, name, content }: { parentId: string; kind: "folder" | "text"; name: string; content?: string }) => {
      if (!activeSpace) throw new Error("No active space");
      return createNode(client, activeSpace.id, { parent_id: parentId, kind, name, content });
    },
    onSuccess: (node) => {
      queryClient.setQueryData(queryKeys.node(node.space_id, node.id), node);
      invalidateNodeLists(queryClient, node.space_id, [node.parent_id]);
      onCreated(node);
    }
  });
}

export function useUpdateNodeMutation(onUpdated: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: ({ node, name }: { node: NodeSummary; name: string }) => updateNode(client, node.space_id, node.id, { name }),
    onSuccess: (node, { node: previousNode }) => {
      updateNodeCaches(queryClient, node, () => node);
      if (node.kind === "folder") {
        invalidateFolderSubtree(queryClient, node.space_id);
      } else {
        invalidateNodeLists(queryClient, node.space_id, [node.parent_id]);
        removeMarkdownImagePreviewQuery(queryClient, node.space_id, previousNode.path);
      }
      onUpdated(node);
    }
  });
}

export function useUpdateNodeSearchPolicyMutation(onUpdated: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ node, enabled }: { node: RestNode; enabled: boolean }) =>
      updateNodeSearchPolicy(client, node.space_id, node.id, enabled),
    onSuccess: (node) => {
      updateNodeCaches(queryClient, node, () => node);
      invalidateRecentNodes(queryClient, node.space_id);
      onUpdated(node);
    }
  });
}

export function useUpdateTextEncryptionMutation(onUpdated: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ node, enabled }: { node: RestNode; enabled: boolean }) =>
      updateTextEncryption(client, node.space_id, node.id, enabled),
    onSuccess: (node) => {
      updateNodeCaches(queryClient, node, () => node);
      invalidateRecentNodes(queryClient, node.space_id);
      invalidateText(queryClient, node.space_id, node.id);
      onUpdated(node);
    }
  });
}

export function useMoveNodeMutation(
  onMoved: (node: RestNode) => void,
  options: MoveNodeMutationOptions = {}
) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: options.silentError ? { silentError: true } : undefined,
    mutationFn: ({ node, parentId }: { node: NodeSummary; parentId: string }) => moveNode(client, node.space_id, node.id, { new_parent_id: parentId, expected_parent_id: node.parent_id }),
    onSuccess: (node, { node: previousNode }) => {
      updateNodeCaches(queryClient, node, () => node);
      if (node.kind === "folder") {
        invalidateFolderSubtree(queryClient, node.space_id);
      } else {
        invalidateNodeLists(queryClient, node.space_id, [previousNode.parent_id, node.parent_id]);
        removeMarkdownImagePreviewQuery(queryClient, node.space_id, previousNode.path);
      }
      onMoved(node);
    }
  });
}

export function useDeleteNodeMutation(onDeleted: (node: NodeSummary) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: ({ node, recursive }: { node: NodeSummary; recursive: boolean }) => deleteNode(client, node.space_id, node.id, recursive).then(() => node),
    onSuccess: async (node, { recursive }) => {
      await removeDeletedNodeQueries(queryClient, node, recursive);
      onDeleted(node);
      if (recursive && node.kind === "folder") {
        invalidateFolderSubtree(queryClient, node.space_id);
      } else {
        invalidateNodeLists(queryClient, node.space_id, [node.parent_id]);
      }
    }
  });
}

export function useReplaceMetadataMutation(onReplaced: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    meta: { silentError: true },
    mutationFn: ({ node, metadata }: { node: RestNode; metadata: Record<string, unknown> }) => replaceMetadata(client, node.space_id, node.id, metadata),
    onSuccess: (node) => {
      updateNodeCaches(queryClient, node, () => node);
      invalidateRecentNodes(queryClient, node.space_id);
      onReplaced(node);
    }
  });
}

export function useRevealNode() {
  const client = useApiClient();
  return (spaceId: string, nodeId: string) => revealNode(client, spaceId, nodeId);
}
