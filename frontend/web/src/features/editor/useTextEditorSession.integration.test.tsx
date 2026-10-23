import { useQueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import { applyExternalFileChanges } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { FileChangeDelta, ReadTextResponse, TextResponse } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { useNodeFreshness } from "./useEditorQueries";
import { useTextEditorSession } from "./useTextEditorSession";

const node = makeRestNode({ content_sha256: "sha-1" });
const textPath = `/api/v1/spaces/${node.space_id}/text/${node.id}`;
const change: FileChangeDelta = {
  id: 1,
  node_id: node.id,
  op_type: "text.replace",
  item_kind: "text",
  affected_parent_ids: [node.parent_id!],
  parent_scope_known: true,
  path_changed: false,
  subtree_changed: false
};

describe("text editing with the shared server cache", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
  });

  it("preserves the editing baseline across external sync until an explicit reload", async () => {
    const server = mockTextServer();
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    act(() => result.current.session.setDraft("local draft"));

    server.change("remote update");
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => expect(result.current.session.content).toBe("remote update"));
    expect(result.current.session.draft).toBe("local draft");

    act(() => result.current.session.saveDraft());
    await waitFor(() => expect(result.current.session.conflict).toBe(true));
    expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({
      content: "local draft",
      expected_sha256: "sha-1"
    }));
    expect(result.current.mode).toBe("edit");
    expect(result.current.session.draft).toBe("local draft");

    act(() => result.current.session.reloadConflict());
    await waitFor(() => {
      expect(result.current.session.draft).toBe("remote update");
      expect(result.current.session.conflict).toBe(false);
      expect(result.current.session.dirty).toBe(false);
    });
    act(() => result.current.session.setDraft("edit after reload"));
    act(() => result.current.session.saveDraft());
    await waitFor(() => expect(result.current.mode).toBe("preview"));
    expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({
      content: "edit after reload",
      expected_sha256: "sha-2"
    }));
  });

  it("updates a clean editor when external sync refreshes the text cache", async () => {
    const server = mockTextServer();
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));

    server.change("remote update");
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => {
      expect(result.current.session.content).toBe("remote update");
      expect(result.current.session.draft).toBe("remote update");
      expect(result.current.session.dirty).toBe(false);
    });
    expect(result.current.mode).toBe("edit");
    expect(server.save).not.toHaveBeenCalled();
  });

  it("waits for an in-flight external text refresh when node metadata arrives first", async () => {
    const server = mockTextServer();
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    const pending = deferred<void>();
    server.read.mockImplementation(async () => {
      await pending.promise;
      return server.readResponse();
    });

    server.change("remote update");
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => expect(result.current.latestNode.content_sha256).toBe("sha-2"));
    expect(result.current.session.textQuery.isFetching).toBe(true);

    await act(async () => { pending.resolve(); });
    await waitFor(() => {
      expect(result.current.session.content).toBe("remote update");
      expect(result.current.session.draft).toBe("remote update");
      expect(result.current.session.textQuery.isFetching).toBe(false);
    });
    expect(server.read).toHaveBeenCalledTimes(2);
    expect(server.save).not.toHaveBeenCalled();
  });

  it("catches up when an in-flight body response predates newer node metadata", async () => {
    const server = mockTextServer();
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    server.change("revision two");
    const bodyTwo = server.readResponse();
    const pending = deferred<Response>();
    server.read.mockImplementationOnce(() => pending.promise);
    act(() => {
      void result.current.queryClient.refetchQueries({ queryKey: queryKeys.text(node.space_id, node.id), exact: true });
    });
    await waitFor(() => expect(server.read).toHaveBeenCalledTimes(2));

    server.change("revision three");
    await act(async () => {
      await result.current.queryClient.refetchQueries({ queryKey: queryKeys.node(node.space_id, node.id), exact: true });
    });
    await waitFor(() => expect(result.current.latestNode.content_sha256).toBe("sha-3"));
    await act(async () => { pending.resolve(bodyTwo); });
    await waitFor(() => {
      expect(result.current.session.textQuery.isFetching).toBe(false);
      expect(result.current.session.content).toBe("revision three");
      expect(result.current.session.draft).toBe("revision three");
    });
    expect(server.read).toHaveBeenCalledTimes(3);
  });

  it("leaves failed body refreshes to the query retry policy", async () => {
    const server = mockTextServer();
    const { result } = renderHook(() => {
      useQueryClient().setQueryDefaults(queryKeys.text(node.space_id, node.id), { retry: false });
      return useEditor();
    }, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    server.read.mockImplementation(async () => jsonResponse({ message: "Temporarily unavailable" }, 503));
    server.change("remote update");
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => expect(result.current.session.textQuery.isFetching).toBe(false));
    expect(server.read).toHaveBeenCalledTimes(2);
    expect(result.current.session.textQuery.isError).toBe(true);
    expect(result.current.session.draft).toBe("original");

    server.read.mockImplementation(async () => server.readResponse());
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => expect(result.current.session.content).toBe("remote update"));
    expect(result.current.session.draft).toBe("remote update");
  });

  it("preserves input started while an external text refresh is pending", async () => {
    const server = mockTextServer();
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    const pending = deferred<void>();
    server.read.mockImplementation(async () => {
      await pending.promise;
      return server.readResponse();
    });
    server.change("remote update");
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => expect(result.current.session.textQuery.isFetching).toBe(true));
    act(() => result.current.session.setDraft("typed during refresh"));
    await act(async () => {
      pending.resolve();
      await pending.promise;
    });
    await waitFor(() => {
      expect(result.current.session.content).toBe("remote update");
      expect(result.current.session.textQuery.isFetching).toBe(false);
    });
    expect(result.current.session.draft).toBe("typed during refresh");
    act(() => result.current.session.saveDraft());
    await waitFor(() => expect(result.current.session.conflict).toBe(true));
    expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({ expected_sha256: "sha-1" }));
  });

  it.each(["submitted draft plus new input", "original"])("preserves post-submit input %j and uses the saved SHA for the next save", async (laterDraft) => {
    const server = mockTextServer();
    const pending = deferred<Response>();
    server.save.mockImplementationOnce(() => pending.promise);
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    act(() => result.current.session.setDraft("submitted draft"));
    act(() => result.current.session.saveDraft());
    await waitFor(() => expect(server.save).toHaveBeenCalledTimes(1));
    expect(result.current.session.saving).toBe(true);
    expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({
      content: "submitted draft",
      expected_sha256: "sha-1"
    }));
    const firstSessionId = server.save.mock.calls[0][0].edit_session_id;
    expect(firstSessionId).toEqual(expect.any(String));

    act(() => result.current.session.setDraft(laterDraft));
    await act(async () => {
      pending.resolve(server.commit("submitted draft"));
      await pending.promise;
    });
    await waitFor(() => {
      expect(result.current.session.saving).toBe(false);
      expect(result.current.session.content).toBe("submitted draft");
    });
    expect(result.current.mode).toBe("edit");
    expect(result.current.session.draft).toBe(laterDraft);
    expect(result.current.session.dirty).toBe(true);
    expect(result.current.session.canSave).toBe(true);

    act(() => result.current.session.saveDraft());
    await waitFor(() => {
      expect(result.current.mode).toBe("preview");
      expect(result.current.session.content).toBe(laterDraft);
      expect(result.current.session.dirty).toBe(false);
    });
    expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({
      content: laterDraft,
      expected_sha256: "sha-2"
    }));
    expect(server.save.mock.lastCall?.[0].edit_session_id).toEqual(expect.any(String));
    expect(server.save.mock.lastCall?.[0].edit_session_id).not.toBe(firstSessionId);
  });

  it.each([
    { refresh: "pending", continuedTyping: false, delayedNode: false },
    { refresh: "completed", continuedTyping: false, delayedNode: false },
    { refresh: "pending", continuedTyping: true, delayedNode: false },
    { refresh: "completed", continuedTyping: true, delayedNode: false },
    { refresh: "completed", continuedTyping: false, delayedNode: true }
  ] as const)("keeps the newer external revision after an older save response ($refresh refresh, continued typing: $continuedTyping, delayed node: $delayedNode)", async ({ refresh, continuedTyping, delayedNode }) => {
    const server = mockTextServer();
    const savedResponse = deferred<Response>();
    server.save.mockImplementationOnce(() => savedResponse.promise);
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    act(() => result.current.session.setDraft("submitted draft"));
    act(() => result.current.session.saveDraft());
    await waitFor(() => expect(server.save).toHaveBeenCalledOnce());

    // The PUT committed first, but its response reaches the browser after a newer write.
    const olderSaveResponse = server.commit("submitted draft");
    if (continuedTyping) act(() => result.current.session.setDraft("continued local input"));
    server.change("newer remote content");
    const externalRead = deferred<Response>();
    const externalReadResponse = server.readResponse();
    server.read.mockImplementationOnce(() => externalRead.promise);
    await act(async () => {
      await applyExternalFileChanges(result.current.queryClient, node.space_id, [change]);
    });
    await waitFor(() => {
      expect(result.current.session.textQuery.isFetching).toBe(true);
      expect(result.current.queryClient.getQueryData(queryKeys.node(node.space_id, node.id))).toMatchObject({ content_sha256: "sha-3" });
    });
    if (refresh === "completed") {
      await act(async () => {
        externalRead.resolve(externalReadResponse);
        await externalRead.promise;
      });
      await waitFor(() => {
        expect(result.current.session.content).toBe("newer remote content");
        expect(result.current.session.textQuery.isFetching).toBe(false);
      });
    }

    const nodeRefresh = delayedNode ? deferred<Response>() : null;
    if (nodeRefresh) server.nodeRead.mockImplementationOnce(() => nodeRefresh.promise);
    await act(async () => {
      savedResponse.resolve(olderSaveResponse);
      await savedResponse.promise;
    });
    if (nodeRefresh) {
      // Exercise text C arriving while the canonical node still describes saved version B.
      await waitFor(() => {
        expect(result.current.mode).toBe("preview");
        expect(result.current.session.content).toBe("newer remote content");
        expect(result.current.session.saving).toBe(true);
        expect(result.current.queryClient.getQueryData(queryKeys.node(node.space_id, node.id))).toMatchObject({ content_sha256: "sha-2" });
      });
      await act(async () => {
        nodeRefresh.resolve(server.nodeResponse());
        await nodeRefresh.promise;
      });
    }
    await waitFor(() => {
      expect(result.current.session.saving).toBe(false);
      expect(result.current.session.content).toBe("newer remote content");
      expect(result.current.queryClient.getQueryData(queryKeys.node(node.space_id, node.id))).toMatchObject({ content_sha256: "sha-3" });
    });
    if (refresh === "pending") {
      // Complete the canceled transport as well; it must not change the visible result.
      await act(async () => {
        externalRead.resolve(externalReadResponse);
        await externalRead.promise;
      });
      expect(result.current.session.content).toBe("newer remote content");
    }
    if (continuedTyping) {
      expect(result.current.mode).toBe("edit");
      expect(result.current.session.draft).toBe("continued local input");
      expect(result.current.session.dirty).toBe(true);
      act(() => result.current.session.saveDraft());
      await waitFor(() => expect(result.current.session.conflict).toBe(true));
      expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({
        content: "continued local input",
        expected_sha256: "sha-2"
      }));
      expect(result.current.session.draft).toBe("continued local input");
      expect(result.current.session.content).toBe("newer remote content");
    } else {
      expect(result.current.mode).toBe("preview");
      expect(result.current.session.dirty).toBe(false);
      await waitFor(() => expect(result.current.session.externalUpdate).toBeNull());
    }
  });

  it("returns to preview after saving an unchanged submitted draft", async () => {
    const server = mockTextServer();
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    act(() => result.current.session.setDraft("saved normally"));
    act(() => result.current.session.saveDraft());

    await waitFor(() => {
      expect(result.current.mode).toBe("preview");
      expect(result.current.session.content).toBe("saved normally");
      expect(result.current.session.dirty).toBe(false);
      expect(result.current.session.conflict).toBe(false);
    });
    expect(server.save).toHaveBeenCalledOnce();
    expect(server.read).toHaveBeenCalledOnce();
  });

  it.each(["success", "conflict"] as const)("keeps a new edit session intact when the canceled session's save returns %s", async (outcome) => {
    const server = mockTextServer();
    const pending = deferred<Response>();
    server.save.mockImplementationOnce(() => pending.promise);
    const { result } = renderHook(useEditor, { wrapper: ApiWrapper });
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    act(() => result.current.session.setDraft("submitted in old session"));
    act(() => result.current.session.saveDraft());
    await waitFor(() => expect(server.save).toHaveBeenCalledOnce());

    act(() => result.current.session.cancelEdit());
    await waitFor(() => expect(result.current.mode).toBe("preview"));
    act(() => result.current.setMode("edit"));
    await waitFor(() => expect(result.current.session.draft).toBe("original"));
    act(() => result.current.session.setDraft("new session draft"));

    await act(async () => {
      pending.resolve(outcome === "success"
        ? server.commit("submitted in old session")
        : jsonResponse({ error: { kind: "conflict", message: "The document changed" } }, 409));
      await pending.promise;
    });
    await waitFor(() => expect(result.current.session.saving).toBe(false));
    expect(result.current.mode).toBe("edit");
    expect(result.current.session.draft).toBe("new session draft");
    expect(result.current.session.dirty).toBe(true);
    expect(result.current.session.conflict).toBe(false);
    expect(result.current.session.canSave).toBe(true);
    expect(result.current.queryClient.getQueryData<ReadTextResponse>(queryKeys.text(node.space_id, node.id))?.text).toMatchObject({
      content: outcome === "success" ? "submitted in old session" : "original",
      content_sha256: outcome === "success" ? "sha-2" : "sha-1"
    });
  });

  it("does not advance a dirty pane's baseline when another pane saves the same document", async () => {
    const server = mockTextServer();
    const { result } = renderHook(() => ({ first: useEditor(), second: useEditor() }), { wrapper: ApiWrapper });
    await waitFor(() => {
      expect(result.current.first.session.draft).toBe("original");
      expect(result.current.second.session.draft).toBe("original");
    });
    act(() => {
      result.current.first.session.setDraft("first pane change");
      result.current.second.session.setDraft("second pane change");
    });
    act(() => result.current.first.session.saveDraft());
    await waitFor(() => expect(result.current.first.mode).toBe("preview"));
    await waitFor(() => expect(result.current.second.session.content).toBe("first pane change"));
    expect(result.current.second.session.draft).toBe("second pane change");

    act(() => result.current.second.session.saveDraft());
    await waitFor(() => expect(result.current.second.session.conflict).toBe(true));
    expect(server.save).toHaveBeenLastCalledWith(expect.objectContaining({
      content: "second pane change",
      expected_sha256: "sha-1"
    }));
    expect(result.current.second.mode).toBe("edit");
    expect(result.current.second.session.draft).toBe("second pane change");
    expect(result.current.first.session.content).toBe("first pane change");
  });
});

function ApiWrapper({ children }: { children: ReactNode }) {
  return <ApiProvider authCacheKey="editor-integration">{children}</ApiProvider>;
}

function useEditor() {
  const [mode, setMode] = useState<"edit" | "preview">("edit");
  const queryClient = useQueryClient();
  const freshness = useNodeFreshness(node);
  const currentNode = freshness.data ?? node;
  const session = useTextEditorSession({
    node: currentNode,
    latestNode: currentNode,
    mode,
    canWrite: true,
    onSetMode: setMode
  });
  return { session, mode, setMode, queryClient, latestNode: currentNode };
}

type SaveRequest = {
  content: string;
  expected_sha256?: string;
  edit_session_id: string;
};

function mockTextServer() {
  let revision = 1;
  let content = "original";
  const currentNode = () => ({
    ...node,
    content_sha256: `sha-${revision}`,
    updated_at: `2026-07-01T00:00:0${revision}Z`
  });
  const text = (): TextResponse["text"] => ({
    node_id: node.id,
    storage_format: "plain",
    content_sha256: currentNode().content_sha256,
    byte_len: content.length,
    line_count: 1,
    updated_by: node.updated_by,
    updated_at: currentNode().updated_at
  });
  const nodeRef = { id: node.id, path: node.path };
  const change = (nextContent: string) => { content = nextContent; revision++; };
  const commit = (nextContent: string) => {
    change(nextContent);
    return jsonResponse({ node: nodeRef, text: text() } satisfies TextResponse);
  };
  const save = vi.fn(async (request: SaveRequest): Promise<Response> => {
    if (request.expected_sha256 !== undefined && request.expected_sha256 !== currentNode().content_sha256) {
      return jsonResponse({ error: { kind: "conflict", message: "The document changed" } }, 409);
    }
    return commit(request.content);
  });
  const readResponse = () => jsonResponse({
    node: nodeRef,
    text: {
      ...text(), storage_format: "plain", content,
      start_line: 1, end_line: 1, returned_lines: 1, truncated: false, next_start_line: null
    }
  } satisfies ReadTextResponse);
  const read = vi.fn(async () => readResponse());
  const nodeResponse = () => jsonResponse(currentNode());
  const nodeRead = vi.fn(async () => nodeResponse());
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    if (path === textPath && init?.method === "PUT") {
      return save(JSON.parse(String(init.body)) as SaveRequest);
    }
    if (path.startsWith(`${textPath}?`) && init?.method === "GET") {
      return read();
    }
    if (path === `/api/v1/spaces/${node.space_id}/nodes/${node.id}` && init?.method === "GET") {
      return nodeRead();
    }
    throw new Error(`Unexpected request: ${init?.method} ${path}`);
  }));
  return { save, read, nodeRead, change, commit, readResponse, nodeResponse };
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
