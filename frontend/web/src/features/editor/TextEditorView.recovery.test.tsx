import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import { applyExternalFileChanges } from "../../api/queryInvalidation";
import { queryKeys } from "../../api/queryKeys";
import type { FileChangeDelta, ReadTextResponse } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { TextEditorView } from "./TextEditorView";
import { useNodeFreshness } from "./useEditorQueries";

const node = makeRestNode({ name: "note.md", content_sha256: "sha-1" });
const textPath = `/api/v1/spaces/${node.space_id}/text/${node.id}`;
const change: FileChangeDelta = {
  id: 1, node_id: node.id, op_type: "text.replace", item_kind: "text",
  affected_parent_ids: [node.parent_id!], parent_scope_known: true,
  path_changed: false, subtree_changed: false
};

it("keeps manual retry available after an initial read and its retry fail", async () => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  const read = vi.fn(async () => jsonResponse({ message: "Unavailable" }, 503));
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
    if (String(input).startsWith(`${textPath}?`)) return read();
    throw new Error(`Unexpected request: ${String(input)}`);
  });
  function Editor() {
    useQueryClient().setQueryDefaults(queryKeys.text(node.space_id, node.id), { retry: false });
    const [mode, setMode] = useState<"edit" | "preview">("edit");
    return <TextEditorView active groupId={0} node={node}
      qualifiedPath="Daily:/note.md" mode={mode} canWriteActiveSpace canOpenInNewGroup canClose={false}
      onSetMode={setMode} onClose={vi.fn()} onOpenNodeInNewGroup={vi.fn()} onOpenMarkdownLink={vi.fn()}
      onRenameNode={vi.fn()} onMoveNode={vi.fn()} onDeleteNode={vi.fn()} />;
  }
  render(<ApiProvider authCacheKey="initial-text-recovery"><Editor /></ApiProvider>);
  const user = userEvent.setup();
  await screen.findByRole("alert");
  expect(read).toHaveBeenCalledTimes(1);

  let finish!: (response: Response) => void;
  read.mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve; }));
  const retry = screen.getByRole("button", { name: "Retry text" });
  await user.click(retry);
  await screen.findByText("Loading text…");
  fireEvent.click(retry);
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => { finish(jsonResponse({ message: "Still unavailable" }, 503)); });
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "Retry text" })).toBeEnabled();
  expect(read).toHaveBeenCalledTimes(2);

  read.mockResolvedValueOnce(jsonResponse({
    node: { id: node.id, path: node.path },
    text: {
      node_id: node.id, storage_format: "plain", content: "recovered", content_sha256: "sha-1",
      byte_len: 9, line_count: 1, start_line: 1, end_line: 1, returned_lines: 1,
      truncated: false, next_start_line: null, updated_by: node.updated_by, updated_at: node.updated_at
    }
  } satisfies ReadTextResponse));
  await user.click(screen.getByRole("button", { name: "Retry text" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Edit text content" })).toHaveValue("recovered"));
  expect(read).toHaveBeenCalledTimes(3);
});

it.each([false, true])("retries a failed text refresh without replacing the editing baseline (dirty: %s)", async (dirty) => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  let queryClient!: QueryClient;
  let content = "original";
  let sha = "sha-1";
  const textResponse = () => jsonResponse({
    node: { id: node.id, path: node.path },
    text: {
      node_id: node.id, storage_format: "plain", content, content_sha256: sha,
      byte_len: content.length, line_count: 1, start_line: 1, end_line: 1,
      returned_lines: 1, truncated: false, next_start_line: null,
      updated_by: node.updated_by, updated_at: node.updated_at
    }
  } satisfies ReadTextResponse);
  const read = vi.fn(async () => textResponse());
  const save = vi.fn(async (request: { content: string; expected_sha256?: string }) => {
    if (request.expected_sha256 !== sha) return jsonResponse({ message: "The document changed", kind: "conflict" }, 409);
    content = request.content;
    sha = "sha-3";
    return textResponse();
  });
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const path = String(input);
    if (path.startsWith(`${textPath}?`)) return read();
    if (path === textPath && init?.method === "PUT") {
      return save(JSON.parse(String(init.body)));
    }
    if (path === `/api/v1/spaces/${node.space_id}/nodes/${node.id}`) return jsonResponse({ ...node, content_sha256: sha });
    throw new Error(`Unexpected request: ${init?.method} ${path}`);
  });

  function Editor() {
    queryClient = useQueryClient();
    // Fix the terminal state without relying on retry backoff timers.
    queryClient.setQueryDefaults(queryKeys.text(node.space_id, node.id), { retry: false });
    const latest = useNodeFreshness(node).data ?? node;
    const [mode, setMode] = useState<"edit" | "preview">("edit");
    return <TextEditorView active groupId={0} node={latest} latestNode={latest}
      qualifiedPath="Daily:/note.md" mode={mode} canWriteActiveSpace canOpenInNewGroup canClose={false}
      onSetMode={setMode} onClose={vi.fn()} onOpenNodeInNewGroup={vi.fn()} onOpenMarkdownLink={vi.fn()}
      onRenameNode={vi.fn()} onMoveNode={vi.fn()} onDeleteNode={vi.fn()} />;
  }

  render(<ApiProvider authCacheKey="text-recovery"><Editor /></ApiProvider>);
  const user = userEvent.setup();
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Edit text content" })).toHaveValue("original"));
  if (dirty) await user.type(screen.getByRole("textbox", { name: "Edit text content" }), " local draft");
  content = "remote update";
  sha = "sha-2";
  read.mockImplementation(async () => jsonResponse({ message: "Unavailable" }, 503));
  await act(async () => { await applyExternalFileChanges(queryClient, node.space_id, [change]); });
  await screen.findByText("Could not load text.");
  expect(read).toHaveBeenCalledTimes(2);

  // A later empty sync has no new change to invalidate the failed document again.
  await act(async () => { await applyExternalFileChanges(queryClient, node.space_id, []); });
  let finish!: (response: Response) => void;
  read.mockImplementation(() => new Promise<Response>((resolve) => { finish = resolve; }));
  const retry = screen.getByRole("button", { name: "Retry text" });
  await user.click(retry);
  await waitFor(() => expect(read).toHaveBeenCalledTimes(3));
  expect(retry).toBeDisabled();
  fireEvent.click(retry);
  expect(read).toHaveBeenCalledTimes(3);
  await act(async () => { finish(textResponse()); });

  const editor = await screen.findByRole("textbox", { name: "Edit text content" });
  await waitFor(() => expect(editor).toHaveValue(dirty ? "original local draft" : "remote update"));
  expect(screen.queryByRole("button", { name: "Retry text" })).not.toBeInTheDocument();
  expect(save).not.toHaveBeenCalled();
  expect(read).toHaveBeenCalledTimes(3);

  if (!dirty) {
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await user.type(editor, " continued edit");
  }
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(save).toHaveBeenLastCalledWith(expect.objectContaining({
    content: dirty ? "original local draft" : "remote update continued edit",
    expected_sha256: dirty ? "sha-1" : "sha-2"
  }));
  if (dirty) {
    await screen.findByText("This document changed elsewhere since you opened it.");
    expect(editor).toHaveValue("original local draft");
  } else await waitFor(() => expect(screen.queryByRole("textbox", { name: "Edit text content" })).not.toBeInTheDocument());
});

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}
