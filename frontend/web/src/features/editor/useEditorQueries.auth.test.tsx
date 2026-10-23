import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";

import { ApiProvider } from "../../api/ApiProvider";
import type { TextResponse } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { useSaveTextDocument } from "./useEditorQueries";

const node = makeRestNode();

beforeEach(() => { useUiStore.setState(useUiStore.getInitialState(), true); });

for (const outcome of ["success", "error"] as const) {
  it(`ignores an old account's late text-save ${outcome} after the authenticated cache changes`, async () => {
    let finish!: (response: Response) => void;
    const pending = new Promise<Response>((resolve) => { finish = resolve; });
    const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(() => pending);
    const onSaved = vi.fn();
    const onConflict = vi.fn();
    function Editor() {
      const mutation = useSaveTextDocument(onSaved, onConflict);
      return <button onClick={() => mutation.mutate({
        node, content: "Owner A saved text", editSessionId: "owner-a-edit", force: false
      })}>Save</button>;
    }
    const view = (owner: string) => <ApiProvider authCacheKey={owner}><Editor key={owner} /></ApiProvider>;
    const { rerender } = render(view("a"));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(fetch).toHaveBeenCalledOnce());

    rerender(view("b"));
    act(() => { useUiStore.getState().resetSessionWorkspace(); });
    const current = useUiStore.getState();
    const result: TextResponse = {
      node: { id: node.id, path: node.path },
      text: {
        node_id: node.id, storage_format: "plain", content_sha256: "saved-sha", byte_len: 18,
        line_count: 1, updated_at: node.updated_at, updated_by: node.updated_by
      }
    };
    await act(async () => {
      finish(new Response(JSON.stringify(outcome === "success" ? result : { message: "Save failed" }), {
        status: outcome === "success" ? 200 : 500
      }));
      await pending;
    });
    expect(onSaved).not.toHaveBeenCalled();
    expect(onConflict).not.toHaveBeenCalled();
    expect(useUiStore.getState().editorGroups).toEqual(current.editorGroups);
    expect(useUiStore.getState().saveState).toBe("idle");
    expect(useUiStore.getState().toast).toBeNull();
  });
}
