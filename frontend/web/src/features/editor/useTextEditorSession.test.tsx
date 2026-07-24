import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { useTextEditorSession } from "./useTextEditorSession";
import { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

vi.mock("./useEditorQueries", () => ({
  useTextDocument: vi.fn(),
  useSaveTextDocument: vi.fn()
}));

const node: RestNode = {
  id: "node-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "note.md",
  kind: "text",
  path: "/note.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  content_sha256: "sha-1",
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

const textResponse = {
  node: { id: node.id, path: node.path },
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
    vi.mocked(useSaveTextDocument).mockReturnValue({ mutate: vi.fn(), isPending: false } as never);
  });

  it("reloads a clean editor once when the server content changes", async () => {
    const updatedResponse = {
      ...textResponse,
      text: { ...textResponse.text, content: "updated", content_sha256: "sha-2" }
    } satisfies ReadTextResponse;
    let currentResponse = textResponse;
    const refetch = vi.fn().mockImplementation(async () => {
      currentResponse = updatedResponse;
      return { data: updatedResponse };
    });
    vi.mocked(useTextDocument).mockImplementation(() => ({
      data: currentResponse,
      isSuccess: true,
      refetch
    }) as never);

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
    vi.mocked(useTextDocument).mockReturnValue({
      data: textResponse,
      isSuccess: true,
      refetch
    } as never);

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
      node: { id: nextNode.id, path: nextNode.path },
      text: { ...textResponse.text, node_id: nextNode.id, content: "next", content_sha256: "sha-b" }
    } satisfies ReadTextResponse;
    let resolveReload: ((value: { data: ReadTextResponse; isError: boolean }) => void) | undefined;
    const reload = new Promise<{ data: ReadTextResponse; isError: boolean }>((resolve) => {
      resolveReload = resolve;
    });
    vi.mocked(useTextDocument).mockImplementation((currentNode) => currentNode.id === node.id ? {
      data: textResponse,
      isSuccess: true,
      refetch: () => reload
    } as never : {
      data: nextResponse,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

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
    vi.mocked(useTextDocument).mockReturnValue({
      data: textResponse,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

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
});
