import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useApiClient } from "../../api/ApiProvider";
import { updateNodeCaches } from "../../api/nodeCache";
import { invalidateFileChangeEvents, invalidateRecentNodes, invalidateSpaceLinks } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import { readText } from "../../api/text";
import { listTextRevisions, readTextRevision, restoreTextRevision } from "../../api/textRevisions";
import type { ReadTextResponse, RestNode } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";

// Revision bodies and the comparison baseline live only while the dialog is open.
const scopedRead = { gcTime: 0, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false } as const;

export function useTextRevisions(node: RestNode, selectedId: string | null, onRestored: () => void) {
  const client = useApiClient();
  const queryClient = useQueryClient();
  const list = useInfiniteQuery({
    ...scopedRead,
    queryKey: queryKeys.textRevisionList(node.space_id, node.id),
    queryFn: ({ pageParam }) => listTextRevisions(client, node.space_id, node.id, pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.page.has_more ? last.page.next_cursor : undefined
  });
  const revisions = [...new Map(list.data?.pages.flatMap((page) => page.revisions).map((r) => [r.id, r])).values()];
  const selected = selectedId === "current" ? undefined : revisions.find((r) => r.id === selectedId) ?? revisions[0];
  const body = useQuery({
    ...scopedRead,
    queryKey: queryKeys.textRevision(node.space_id, node.id, selected?.id ?? "none"),
    queryFn: () => readTextRevision(client, node.space_id, node.id, selected!.id),
    enabled: !!selected && !list.isError
  });
  const baseline = useQuery({
    ...scopedRead,
    // Keep the comparison stable until the user explicitly reloads; editor cache refreshes are separate.
    queryKey: queryKeys.textRevisionBaseline(node.space_id, node.id),
    queryFn: () => readText(client, node.space_id, node.id),
    staleTime: Infinity
  });
  const restore = useMutation({
    meta: { silentError: true },
    mutationFn: async ({ revisionId, sha, content }: { revisionId: string; sha: string; content: string }) => ({
      result: await restoreTextRevision(client, node.space_id, node.id, revisionId, sha),
      content
    }),
    onSuccess: async ({ result, content }) => {
      const textKey = queryKeys.text(node.space_id, node.id);
      // Discard any older in-flight read, then publish the restored body before reopening the editor.
      await queryClient.cancelQueries({ queryKey: textKey, exact: true });
      const previous = queryClient.getQueryData<ReadTextResponse>(textKey) ?? baseline.data;
      if (previous) queryClient.setQueryData<ReadTextResponse>(textKey, {
        ...previous,
        text: {
          ...previous.text,
          ...result,
          storage_format: "plain",
          content,
          start_line: 1,
          end_line: result.line_count,
          returned_lines: result.line_count,
          truncated: false,
          next_start_line: null
        }
      });
      const updated = { ...node, ...result, id: node.id };
      updateNodeCaches(queryClient, updated, (previous) => ({ ...previous, content_sha256: result.content_sha256, byte_len: result.byte_len, line_count: result.line_count }));
      useUiStore.getState().updateGroupsNode(updated);
      void queryClient.invalidateQueries({ queryKey: textKey, exact: true });
      void queryClient.invalidateQueries({ queryKey: queryKeys.node(node.space_id, node.id), exact: true });
      void queryClient.invalidateQueries({ queryKey: queryKeys.textRevisionList(node.space_id, node.id) });
      invalidateRecentNodes(queryClient, node.space_id);
      invalidateSpaceLinks(queryClient, node.space_id);
      invalidateFileChangeEvents(queryClient, node.space_id);
      useUiStore.getState().showToast("Version restored. The previous saved content remains in history.");
      onRestored();
    }
  });
  return { list, revisions, selected, body, baseline, restore };
}
