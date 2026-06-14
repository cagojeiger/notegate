import { useMutation, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { uploadFile } from "../../api/files";
import { replaceMetadata } from "../../api/metadata";
import { createNode, deleteNode, moveNode, revealNode, updateNode } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode, Space } from "../../api/types";
import { useInvalidateSpace } from "./useWorkbenchCache";

export function useCreateNodeMutation(activeSpace: Space | null, onCreated: (node: RestNode) => void) {
  const client = useApiClient();
  const invalidateSpace = useInvalidateSpace();
  return useMutation({
    mutationFn: ({ parentId, kind, name, content }: { parentId: string; kind: "folder" | "text"; name: string; content?: string }) => {
      if (!activeSpace) throw new Error("No active space");
      return createNode(client, activeSpace.id, { parent_id: parentId, kind, name, content });
    },
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      onCreated(node);
    }
  });
}

export function useUpdateNodeMutation(onUpdated: (node: RestNode) => void) {
  const client = useApiClient();
  const invalidateSpace = useInvalidateSpace();
  return useMutation({
    mutationFn: ({ node, name }: { node: RestNode; name: string }) => updateNode(client, node.space_id, node.id, { name }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      onUpdated(node);
    }
  });
}

export function useMoveNodeMutation(onMoved: (node: RestNode) => void) {
  const client = useApiClient();
  const invalidateSpace = useInvalidateSpace();
  return useMutation({
    mutationFn: ({ node, parentId }: { node: RestNode; parentId: string }) => moveNode(client, node.space_id, node.id, { new_parent_id: parentId, expected_parent_id: node.parent_id }),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      onMoved(node);
    }
  });
}

export function useDeleteNodeMutation(onDeleted: (node: RestNode) => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const invalidateSpace = useInvalidateSpace();
  return useMutation({
    mutationFn: ({ node, recursive }: { node: RestNode; recursive: boolean }) => deleteNode(client, node.space_id, node.id, recursive).then(() => node),
    onSuccess: (node) => {
      void queryClient.cancelQueries({ queryKey: queryKeys.text(node.space_id, node.id) });
      void queryClient.cancelQueries({ queryKey: queryKeys.file(node.space_id, node.id) });
      void queryClient.cancelQueries({ queryKey: queryKeys.metadata(node.space_id, node.id) });
      queryClient.removeQueries({ queryKey: queryKeys.text(node.space_id, node.id) });
      queryClient.removeQueries({ queryKey: queryKeys.file(node.space_id, node.id) });
      queryClient.removeQueries({ queryKey: queryKeys.metadata(node.space_id, node.id) });
      onDeleted(node);
      invalidateSpace(node.space_id);
    }
  });
}

export function useUploadFileMutation(activeSpace: Space | null, onUploaded: (node: RestNode) => void) {
  const client = useApiClient();
  const invalidateSpace = useInvalidateSpace();
  return useMutation({
    mutationFn: ({ parentId, name, file }: { parentId: string; name: string; file: File }) => {
      if (!activeSpace) throw new Error("No active space");
      return uploadFile(client, activeSpace.id, { parentNodeId: parentId, name, file });
    },
    onSuccess: (response) => {
      invalidateSpace(response.node.space_id);
      onUploaded(response.node);
    }
  });
}

export function useReplaceMetadataMutation(onReplaced: (node: RestNode) => void) {
  const client = useApiClient();
  const invalidateSpace = useInvalidateSpace();
  return useMutation({
    mutationFn: ({ node, metadata }: { node: RestNode; metadata: Record<string, unknown> }) => replaceMetadata(client, node.space_id, node.id, metadata),
    onSuccess: (node) => {
      invalidateSpace(node.space_id);
      onReplaced(node);
    }
  });
}

export function useRevealNode() {
  const client = useApiClient();
  return (spaceId: string, nodeId: string) => revealNode(client, spaceId, nodeId);
}
