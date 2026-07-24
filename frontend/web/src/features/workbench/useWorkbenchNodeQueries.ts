import { useMutation, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { replaceMetadata } from "../../api/metadata";
import { updateNodeCaches } from "../../api/nodeCache";
import { createNode, deleteNode, moveNode, revealNode, updateNode } from "../../api/nodes";
import {
  invalidateFolderSubtree,
  invalidateNodeLists,
  removeDeletedNodeQueries,
  removeMarkdownImageNodeQueries
} from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode, Space } from "../../api/types";

export function useCreateNodeMutation(activeSpace: Space | null, onCreated: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
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
    mutationFn: ({ node, name }: { node: RestNode; name: string }) => updateNode(client, node.space_id, node.id, { name }),
    onSuccess: (node) => {
      updateNodeCaches(queryClient, node, () => node);
      if (node.kind === "folder") {
        invalidateFolderSubtree(queryClient, node.space_id);
      } else {
        invalidateNodeLists(queryClient, node.space_id, [node.parent_id]);
        removeMarkdownImageNodeQueries(queryClient, node.space_id);
      }
      onUpdated(node);
    }
  });
}

export function useMoveNodeMutation(onMoved: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ node, parentId }: { node: RestNode; parentId: string }) => moveNode(client, node.space_id, node.id, { new_parent_id: parentId, expected_parent_id: node.parent_id }),
    onSuccess: (node, { node: previousNode }) => {
      updateNodeCaches(queryClient, node, () => node);
      if (node.kind === "folder") {
        invalidateFolderSubtree(queryClient, node.space_id);
      } else {
        invalidateNodeLists(queryClient, node.space_id, [previousNode.parent_id, node.parent_id]);
        removeMarkdownImageNodeQueries(queryClient, node.space_id);
      }
      onMoved(node);
    }
  });
}

export function useDeleteNodeMutation(onDeleted: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ node, recursive }: { node: RestNode; recursive: boolean }) => deleteNode(client, node.space_id, node.id, recursive).then(() => node),
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
    mutationFn: ({ node, metadata }: { node: RestNode; metadata: Record<string, unknown> }) => replaceMetadata(client, node.space_id, node.id, metadata),
    onSuccess: (node) => {
      updateNodeCaches(queryClient, node, () => node);
      invalidateNodeLists(queryClient, node.space_id, [node.parent_id]);
      onReplaced(node);
    }
  });
}

export function useRevealNode() {
  const client = useApiClient();
  return (spaceId: string, nodeId: string) => revealNode(client, spaceId, nodeId);
}
