import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { queryKeys } from "../../api/queryKeys";
import { replaceText } from "../../api/text";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode } from "../../test/fixtures";
import { useSaveTextDocument } from "./useEditorQueries";

const mockClient = vi.hoisted(() => ({}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => mockClient
}));

vi.mock("../../api/text", () => ({
  readText: vi.fn(),
  replaceText: vi.fn()
}));

const node = makeRestNode({ content_sha256: "sha-1" });

describe("useSaveTextDocument", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(replaceText).mockReset();
  });

  it("surfaces a write-lock rejection and refreshes the node lock state", async () => {
    const message = "changes are blocked because the node or an ancestor is write-locked";
    vi.mocked(replaceText).mockRejectedValue(
      new ApiError(message, 423, "node_write_locked")
    );
    const queryClient = new QueryClient({
      defaultOptions: {
        mutations: { retry: false },
        queries: { retry: false }
      }
    });
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
