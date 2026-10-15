import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { removeDeletedNodeQueries } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { ReadTextResponse, TextResponse } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import { useNodeFreshness, useSaveTextDocument, useTextDocument } from "./useEditorQueries";
import { useTextEditorSession } from "./useTextEditorSession";

const client = vi.hoisted(() => ({ get: vi.fn(), put: vi.fn() }));

// Keep the real text adapter and QueryClient cancellation/deduplication behavior.
vi.mock("../../api/ApiProvider", () => ({ useApiClient: () => client }));

const original = "A".repeat(2_048);
const originalSha = "a".repeat(64);
const savedSha = "b".repeat(64);
const node = makeRestNode({ content_sha256: originalSha });
const textKey = queryKeys.text(node.space_id, node.id);
const textPath = `/api/v1/spaces/${node.space_id}/text/${node.id}`;
const latest = {
  updated_at: "2026-10-15T12:00:00Z",
  updated_by: { ...node.updated_by, id: "user-2", display_name: "Another user" }
};

describe("conditional editor text revalidation", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    client.get.mockReset();
    client.put.mockReset();
  });

  it("deduplicates shared observers and keeps the complete body with fresh metadata", async () => {
    const { queryClient, wrapper } = harness();
    const pending = deferred<ReadTextResponse>();
    client.get.mockReturnValue(pending.promise);
    const { result } = renderHook(() => ({ first: useTextDocument(node), second: useTextDocument(node) }), { wrapper });
    expect(client.get).not.toHaveBeenCalled();

    let refreshing!: Promise<void>;
    act(() => { refreshing = queryClient.invalidateQueries({ queryKey: textKey, exact: true }); });
    await waitFor(() => {
      expect(client.get).toHaveBeenCalledOnce();
      expect(result.current.first.isFetching).toBe(true);
      expect(result.current.second.isFetching).toBe(true);
    });
    expectConditionalRequest();
    await act(async () => {
      pending.resolve(unchanged());
      await refreshing;
    });

    const expected = {
      node: { id: node.id, path: "/renamed.md" },
      text: { ...fullText().text, ...latest }
    };
    expect(queryClient.getQueryData(textKey)).toEqual(expected);
    await waitFor(() => {
      expect(result.current.first.data).toEqual(expected);
      expect(result.current.second.data).toEqual(expected);
    });
    expect(client.get).toHaveBeenCalledOnce();
  });

  it("does not let a canceled conditional response overwrite a successful save", async () => {
    const { queryClient, wrapper } = harness();
    const pending = deferred<ReadTextResponse>();
    client.get.mockReturnValue(pending.promise);
    const savedContent = "B".repeat(2_048);
    client.put.mockResolvedValue(writeResponse(savedContent));
    const onSaved = vi.fn();
    const { result } = renderHook(() => ({
      text: useTextDocument(node),
      save: useSaveTextDocument(onSaved, vi.fn())
    }), { wrapper });

    let refreshing!: Promise<unknown>;
    act(() => { refreshing = result.current.text.refetch(); });
    await waitFor(() => expect(client.get).toHaveBeenCalledOnce());
    expectConditionalRequest();
    await act(async () => {
      await result.current.save.mutateAsync({
        node, content: savedContent, expectedSha256: originalSha,
        editSessionId: "session-1", force: false
      });
    });
    expect(onSaved).toHaveBeenCalledOnce();
    const savedCache = queryClient.getQueryData<ReadTextResponse>(textKey);
    expect(savedCache?.text).toMatchObject({ content: savedContent, content_sha256: savedSha });

    await act(async () => {
      pending.resolve(unchanged());
      await pending.promise;
      await refreshing;
    });
    expect(queryClient.getQueryData(textKey)).toEqual(savedCache);
    await waitFor(() => expect(result.current.text.data).toEqual(savedCache));
    expect(client.get).toHaveBeenCalledOnce();
    expect(client.put).toHaveBeenCalledWith(textPath, {
      storage_format: "plain", content: savedContent,
      expected_sha256: originalSha, edit_session_id: "session-1"
    });
  });

  it("does not recreate a removed document cache when its conditional response arrives", async () => {
    const { queryClient, wrapper } = harness();
    const pending = deferred<ReadTextResponse>();
    client.get.mockReturnValue(pending.promise);
    const { result, unmount } = renderHook(() => useTextDocument(node), { wrapper });
    let refreshing!: Promise<unknown>;
    act(() => { refreshing = result.current.refetch(); });
    await waitFor(() => expect(client.get).toHaveBeenCalledOnce());
    expectConditionalRequest();

    unmount();
    await removeDeletedNodeQueries(queryClient, node, false);
    expect(queryClient.getQueryState(textKey)).toBeUndefined();
    await act(async () => {
      pending.resolve(unchanged());
      await pending.promise;
      await refreshing;
    });
    expect(queryClient.getQueryState(textKey)).toBeUndefined();
    expect(client.get).toHaveBeenCalledOnce();
  });

  it("reads a complete body after cache eviction even when the node still has its hash", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.removeQueries({ queryKey: textKey, exact: true });
    client.get.mockResolvedValue(fullText());
    const { result } = renderHook(() => useTextDocument(node), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(client.get).toHaveBeenCalledOnce();
    const url = new URL(client.get.mock.calls[0][0] as string, "http://localhost");
    expect(url.pathname).toBe(textPath);
    expect(url.searchParams.has("if_none_match_sha256")).toBe(false);
    expect(result.current.data).toEqual(fullText());
  });

  it("keeps a dirty draft and its save baseline after a same-body metadata refresh", async () => {
    const { queryClient, wrapper } = harness();
    queryClient.setQueryData(queryKeys.node(node.space_id, node.id), node);
    client.get.mockResolvedValue(unchanged());
    const draft = `${original} local edit`;
    client.put.mockResolvedValue(writeResponse(draft));
    const { result } = renderHook(() => {
      const [mode, setMode] = useState<"edit" | "preview">("edit");
      // Match OpenedNodeGuard: saves also advance the canonical node observed by the editor.
      const freshness = useNodeFreshness(node);
      const currentNode = freshness.data ?? node;
      const session = useTextEditorSession({ node: currentNode, latestNode: currentNode, mode, canWrite: true, onSetMode: setMode });
      return { session, mode, latestNode: currentNode };
    }, { wrapper });
    await waitFor(() => expect(result.current.session.draft).toBe(original));
    act(() => result.current.session.setDraft(draft));
    await act(async () => { await result.current.session.textQuery.refetch(); });
    expectConditionalRequest();
    await waitFor(() => expect(result.current.session.textQuery.data?.text).toMatchObject(latest));
    expect(result.current.session.content).toBe(original);
    expect(result.current.session.draft).toBe(draft);
    expect(result.current.session.dirty).toBe(true);
    expect(result.current.session.externalUpdate).toBeNull();
    expect(result.current.mode).toBe("edit");

    act(() => result.current.session.saveDraft());
    await waitFor(() => {
      expect(result.current.mode).toBe("preview");
      expect(result.current.latestNode.content_sha256).toBe(savedSha);
    });
    expect(client.put).toHaveBeenCalledWith(textPath, expect.objectContaining({
      content: draft, expected_sha256: originalSha
    }));
    expect(client.get).toHaveBeenCalledOnce();
    expect(client.get.mock.calls.map(([path]) => new URL(path as string, "http://localhost").pathname)).toEqual([textPath]);
  });
});

function harness() {
  const queryClient = createTestQueryClient({ defaultOptions: { queries: { staleTime: Infinity } } });
  queryClient.setQueryData(textKey, fullText());
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, wrapper };
}

function fullText(): ReadTextResponse {
  return {
    node: { id: node.id, path: node.path },
    text: {
      node_id: node.id, storage_format: "plain", content: original,
      content_sha256: originalSha, byte_len: original.length, line_count: 1,
      start_line: 1, end_line: 1, returned_lines: 1, truncated: false, next_start_line: null,
      updated_at: node.updated_at, updated_by: node.updated_by
    }
  };
}

function unchanged(): ReadTextResponse {
  return {
    node: { id: node.id, path: "/renamed.md" },
    text: {
      node_id: node.id, storage_format: "plain", unchanged: true, content_returned: false,
      content_sha256: originalSha, byte_len: original.length, line_count: 1, ...latest
    }
  };
}

function writeResponse(content: string): TextResponse {
  return {
    node: { id: node.id, path: node.path },
    text: {
      node_id: node.id, storage_format: "plain", content_sha256: savedSha,
      byte_len: content.length, line_count: 1, ...latest
    }
  };
}

function expectConditionalRequest() {
  const url = new URL(client.get.mock.calls[0][0] as string, "http://localhost");
  expect(url.pathname).toBe(textPath);
  expect(url.searchParams.get("if_none_match_sha256")).toBe(originalSha);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
