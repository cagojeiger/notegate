import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { listChildren } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import { readText, replaceText } from "../../api/text";
import type { ReadTextResponse } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { useNodeChildrenQuery } from "../nodes/useNodeQueries";
import {
  useFolderChildrenStat,
  useSaveTextDocument,
  useTextDocument
} from "./useEditorQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/text", () => ({
  readText: vi.fn(),
  replaceText: vi.fn()
}));

vi.mock("../../api/nodes", () => ({
  getNode: vi.fn(),
  listChildren: vi.fn()
}));

const node = makeRestNode({ content_sha256: "sha-1" });

describe("editor queries", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(listChildren).mockReset();
    vi.mocked(readText).mockReset();
    vi.mocked(replaceText).mockReset();
  });

  it("shares the canonical children request with the tree while keeping first-page count semantics", async () => {
    const folder = makeRestNode({
      id: "folder-1",
      kind: "folder",
      name: "Policies",
      path: "/Policies"
    });
    const firstPage = {
      parent: { id: folder.id, path: folder.path },
      children: [
        makeRestNode({ id: "child-1", parent_id: folder.id }),
        makeRestNode({ id: "child-2", parent_id: folder.id })
      ],
      page: {
        limit: 100,
        returned: 2,
        has_more: true,
        next_cursor: "next-page"
      }
    };
    vi.mocked(listChildren).mockResolvedValue(firstPage);
    const queryClient = createTestQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(
      () => ({
        tree: useNodeChildrenQuery(folder.space_id, folder.id, true),
        stat: useFolderChildrenStat(folder)
      }),
      { wrapper }
    );

    await waitFor(() => {
      expect(result.current.tree.isSuccess).toBe(true);
      expect(result.current.stat.isSuccess).toBe(true);
    });

    expect(listChildren).toHaveBeenCalledTimes(1);
    expect(listChildren).toHaveBeenCalledWith(
      mockClient,
      folder.space_id,
      folder.id,
      null
    );
    expect(result.current.stat.data).toEqual(firstPage);
    expect(queryClient.getQueryData(queryKeys.children(folder.space_id, folder.id))).toEqual({
      pages: [firstPage],
      pageParams: [null]
    });
    expect(
      queryClient.getQueryData([...queryKeys.children(folder.space_id, folder.id), "stat"])
    ).toBeUndefined();
  });

  it("writes a successful plain-text save through to the canonical text cache without refetching", async () => {
    const queryClient = createTestQueryClient({
      defaultOptions: { queries: { staleTime: Number.POSITIVE_INFINITY } }
    });
    const oldText: ReadTextResponse = {
      node: { id: node.id, path: node.path },
      text: {
        node_id: node.id,
        storage_format: "plain",
        content: "before",
        content_sha256: "sha-1",
        byte_len: 6,
        line_count: 1,
        start_line: 1,
        end_line: 1,
        returned_lines: 1,
        truncated: false,
        next_start_line: null,
        updated_by: node.updated_by,
        updated_at: node.updated_at
      }
    };
    queryClient.setQueryData(queryKeys.text(node.space_id, node.id), oldText);
    vi.mocked(readText).mockResolvedValue(oldText);
    vi.mocked(replaceText).mockResolvedValue({
      node: { id: node.id, path: node.path },
      text: {
        node_id: node.id,
        storage_format: "plain",
        content_sha256: "sha-2",
        byte_len: 14,
        line_count: 2,
        updated_by: node.updated_by,
        updated_at: "2026-07-29T12:00:00Z"
      }
    });
    const resetQueries = vi.spyOn(queryClient, "resetQueries");
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const onSaved = vi.fn();
    const onConflict = vi.fn();
    const draft = "changed\nagain\n";
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({
        text: useTextDocument(node),
        save: useSaveTextDocument(node, draft, "sha-1", onSaved, onConflict)
      }),
      { wrapper }
    );

    expect(result.current.text.data).toEqual(oldText);
    expect(readText).not.toHaveBeenCalled();

    act(() => result.current.save.mutate(false));

    await waitFor(() => expect(result.current.save.isSuccess).toBe(true));
    expect(readText).not.toHaveBeenCalled();
    expect(replaceText).toHaveBeenCalledWith(
      mockClient,
      node.space_id,
      node.id,
      draft,
      "sha-1"
    );
    expect(queryClient.getQueryData(queryKeys.text(node.space_id, node.id))).toEqual({
      node: { id: node.id, path: node.path },
      text: {
        node_id: node.id,
        storage_format: "plain",
        content: draft,
        content_sha256: "sha-2",
        byte_len: 14,
        line_count: 2,
        start_line: 1,
        end_line: 2,
        returned_lines: 2,
        truncated: false,
        next_start_line: null,
        updated_by: node.updated_by,
        updated_at: "2026-07-29T12:00:00Z"
      }
    });
    expect(queryClient.getQueryState(queryKeys.text(node.space_id, node.id))?.isInvalidated).toBe(false);
    expect(queryClient.getQueryData(queryKeys.node(node.space_id, node.id))).toMatchObject({
      content_sha256: "sha-2",
      byte_len: 14,
      line_count: 2,
      updated_at: "2026-07-29T12:00:00Z"
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.recent(node.space_id),
      exact: true
    });
    expect(resetQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkStatuses(node.space_id)
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.linkLists(node.space_id),
      refetchType: "none"
    });
    expect(useUiStore.getState().saveState).toBe("saved");
    expect(useUiStore.getState().toast).toBe("Saved");
    expect(onSaved).toHaveBeenCalledTimes(1);
    expect(onConflict).not.toHaveBeenCalled();
  });

  it("surfaces a write-lock rejection and refreshes the node lock state", async () => {
    const message = "changes are blocked because the node or an ancestor is write-locked";
    vi.mocked(replaceText).mockRejectedValue(
      new ApiError(message, 423, "node_write_locked")
    );
    const queryClient = createTestQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const onSaved = vi.fn();
    const onConflict = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () => useSaveTextDocument(node, "changed", "sha-1", onSaved, onConflict),
      { wrapper }
    );

    act(() => result.current.mutate(false));

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(useUiStore.getState().saveState).toBe("error");
    expect(useUiStore.getState().toast).toBe(message);
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: queryKeys.node(node.space_id, node.id),
      exact: true
    });
    expect(onConflict).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });
});
