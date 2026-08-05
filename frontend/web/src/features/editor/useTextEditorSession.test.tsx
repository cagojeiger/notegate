import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { makeRestNode } from "../../test/fixtures";
import { useTextEditorSession } from "./useTextEditorSession";
import type { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

type TextDocumentQuery = ReturnType<typeof useTextDocument>;
type TextDocumentRefetchResult = Pick<
  Awaited<ReturnType<TextDocumentQuery["refetch"]>>,
  "data" | "isError"
>;
type TextDocumentQueryMock = Pick<
  TextDocumentQuery,
  "data" | "isSuccess"
> & {
  refetch: () => Promise<TextDocumentRefetchResult>;
};
type SaveTextMutation = ReturnType<typeof useSaveTextDocument>;
type SaveTextMutationMock = Pick<SaveTextMutation, "mutate" | "isPending">;

const editorQueryMocks = vi.hoisted(() => ({
  useTextDocument: vi.fn<
    (...args: Parameters<typeof useTextDocument>) => TextDocumentQueryMock
  >(),
  useSaveTextDocument: vi.fn<
    (...args: Parameters<typeof useSaveTextDocument>) => SaveTextMutationMock
  >()
}));

vi.mock("./useEditorQueries", () => ({
  useTextDocument: editorQueryMocks.useTextDocument,
  useSaveTextDocument: editorQueryMocks.useSaveTextDocument
}));

const node = makeRestNode({ content_sha256: "sha-1" });

const textResponse = {
  node: { id: node.id, path: node.path, revision: node.revision },
  text: {
    node_id: node.id,
    storage_format: "plain",
    content: "original",
    content_sha256: "sha-1",
    byte_len: 8,
    line_count: 1,
    start_line: 1,
    end_line: 1,
    returned_lines: 1,
    truncated: false,
    next_start_line: null,
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_at: "2026-06-13T00:00:00Z"
  }
} satisfies ReadTextResponse;

describe("useTextEditorSession", () => {
  beforeEach(() => {
    editorQueryMocks.useSaveTextDocument.mockReturnValue({
      mutate: vi.fn(),
      isPending: false
    });
  });

  it("reloads a clean editor once when the server content changes", async () => {
    const updatedResponse = {
      ...textResponse,
      text: { ...textResponse.text, content: "updated", content_sha256: "sha-2" }
    } satisfies ReadTextResponse;
    let currentResponse = textResponse;
    const refetch = vi.fn().mockImplementation(async () => {
      currentResponse = updatedResponse;
      return { data: updatedResponse, isError: false };
    });
    editorQueryMocks.useTextDocument.mockImplementation(() => ({
      data: currentResponse,
      isSuccess: true,
      refetch
    }));

    const { result, rerender } = renderHook(
      ({ latestNode }: { latestNode?: RestNode }) => useTextEditorSession({
        node,
        latestNode,
        mode: "edit",
        canWrite: true,
        onSetMode: vi.fn()
      }),
      { initialProps: { latestNode: undefined as RestNode | undefined } }
    );

    await waitFor(() => expect(result.current.draft).toBe("original"));
    rerender({ latestNode: { ...node, content_sha256: "sha-2" } });

    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(result.current.draft).toBe("updated"));
    rerender({ latestNode: { ...node, content_sha256: "sha-2" } });
    expect(refetch).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(result.current.externalUpdate).toBeNull());
  });

  it("retries the same external sha after a reload failure", async () => {
    const updatedResponse = {
      ...textResponse,
      text: { ...textResponse.text, content: "updated", content_sha256: "sha-2" }
    } satisfies ReadTextResponse;
    const refetch = vi.fn()
      .mockResolvedValueOnce({ data: textResponse, isError: true })
      .mockResolvedValueOnce({ data: updatedResponse, isError: false });
    editorQueryMocks.useTextDocument.mockReturnValue({
      data: textResponse,
      isSuccess: true,
      refetch
    });

    const { result, rerender } = renderHook(
      ({ latestNode }: { latestNode?: RestNode }) => useTextEditorSession({
        node,
        latestNode,
        mode: "edit",
        canWrite: true,
        onSetMode: vi.fn()
      }),
      { initialProps: { latestNode: undefined as RestNode | undefined } }
    );

    await waitFor(() => expect(result.current.draft).toBe("original"));
    rerender({ latestNode: { ...node, content_sha256: "sha-2" } });
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));

    rerender({ latestNode: { ...node, content_sha256: "sha-2" } });
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.draft).toBe("updated"));
  });

  it("ignores a reload that finishes after the editor changes nodes", async () => {
    const nextNode = { ...node, id: "node-2", name: "next.md", path: "/next.md", content_sha256: "sha-b" };
    const nextResponse = {
      node: { id: nextNode.id, path: nextNode.path, revision: nextNode.revision },
      text: { ...textResponse.text, node_id: nextNode.id, content: "next", content_sha256: "sha-b" }
    } satisfies ReadTextResponse;
    let resolveReload: ((value: { data: ReadTextResponse; isError: boolean }) => void) | undefined;
    const reload = new Promise<{ data: ReadTextResponse; isError: boolean }>((resolve) => {
      resolveReload = resolve;
    });
    editorQueryMocks.useTextDocument.mockImplementation((currentNode) =>
      currentNode.id === node.id
        ? {
            data: textResponse,
            isSuccess: true,
            refetch: () => reload
          }
        : textDocumentQuery(nextResponse)
    );

    const { result, rerender } = renderHook(
      ({ currentNode, mode }: { currentNode: RestNode; mode: "preview" | "edit" }) => useTextEditorSession({
        node: currentNode,
        mode,
        canWrite: true,
        onSetMode: vi.fn()
      }),
      { initialProps: { currentNode: node, mode: "edit" as "preview" | "edit" } }
    );

    await waitFor(() => expect(result.current.draft).toBe("original"));
    act(() => result.current.reloadConflict());
    rerender({ currentNode: nextNode, mode: "preview" });
    rerender({ currentNode: nextNode, mode: "edit" });
    await waitFor(() => expect(result.current.draft).toBe("next"));

    await act(async () => {
      resolveReload?.({
        data: { ...textResponse, text: { ...textResponse.text, content: "late old content" } },
        isError: false
      });
      await reload;
    });
    expect(result.current.draft).toBe("next");
  });

  it("suppresses a dismissed external update until a newer sha arrives", async () => {
    editorQueryMocks.useTextDocument.mockReturnValue(textDocumentQuery());

    const { result, rerender } = renderHook(
      ({ latestNode }: { latestNode?: RestNode }) => useTextEditorSession({
        node,
        latestNode,
        mode: "edit",
        canWrite: true,
        onSetMode: vi.fn()
      }),
      { initialProps: { latestNode: undefined as RestNode | undefined } }
    );

    await waitFor(() => expect(result.current.draft).toBe("original"));
    act(() => result.current.setDraft("local edit"));
    rerender({ latestNode: { ...node, content_sha256: "sha-2" } });
    await waitFor(() => expect(result.current.externalUpdate?.content_sha256).toBe("sha-2"));

    act(() => result.current.dismissExternalUpdate());
    rerender({ latestNode: { ...node, content_sha256: "sha-2" } });
    expect(result.current.externalUpdate).toBeNull();

    rerender({ latestNode: { ...node, content_sha256: "sha-3" } });
    await waitFor(() => expect(result.current.externalUpdate?.content_sha256).toBe("sha-3"));
  });

  it("leaves edit mode when the node becomes effectively write-locked", async () => {
    const onSetMode = vi.fn();
    editorQueryMocks.useTextDocument.mockReturnValue(textDocumentQuery());

    const { result } = renderHook(() => useTextEditorSession({
      node: { ...node, effective_write_locked: true },
      mode: "edit",
      canWrite: true,
      onSetMode
    }));

    expect(result.current.canEdit).toBe(false);
    await waitFor(() => expect(onSetMode).toHaveBeenCalledWith("preview"));
  });

  it("preserves an unsaved draft when a lock arrives during editing", async () => {
    const onSetMode = vi.fn();
    editorQueryMocks.useTextDocument.mockReturnValue(textDocumentQuery());

    const { result, rerender } = renderHook(
      ({ currentNode }: { currentNode: RestNode }) => useTextEditorSession({
        node: currentNode,
        mode: "edit",
        canWrite: true,
        onSetMode
      }),
      { initialProps: { currentNode: node } }
    );
    await waitFor(() => expect(result.current.draft).toBe("original"));
    act(() => result.current.setDraft("unsaved"));

    rerender({ currentNode: { ...node, effective_write_locked: true } });

    expect(result.current.draft).toBe("unsaved");
    expect(result.current.dirty).toBe(true);
    expect(result.current.canSave).toBe(false);
    expect(onSetMode).not.toHaveBeenCalled();

    rerender({ currentNode: node });
    expect(result.current.draft).toBe("unsaved");
    expect(result.current.canSave).toBe(true);
  });
});

function textDocumentQuery(
  data: ReadTextResponse = textResponse
): TextDocumentQueryMock {
  return {
    data,
    isSuccess: true,
    refetch: vi.fn().mockResolvedValue({ data, isError: false })
  };
}
