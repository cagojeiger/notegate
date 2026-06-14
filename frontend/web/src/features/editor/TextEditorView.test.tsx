import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { TextEditorView } from "./TextEditorView";
import { useSaveTextDocument, useTextDocument } from "./useEditorQueries";

vi.mock("./useEditorQueries", () => ({
  useTextDocument: vi.fn(),
  useSaveTextDocument: vi.fn()
}));

const node: RestNode = {
  id: "node-1",
  space_id: "space-1",
  parent_id: "root-1",
  name: "large.md",
  kind: "text",
  path: "/large.md",
  sort_order: 0,
  metadata: {},
  has_children: false,
  created_by: { id: "user-1", kind: "user", display_name: "User" },
  updated_by: { id: "user-1", kind: "user", display_name: "User" },
  created_at: "2026-06-13T00:00:00Z",
  updated_at: "2026-06-13T00:00:00Z"
};

const partialText: ReadTextResponse = {
  node: { id: node.id, path: node.path },
  text: {
    node_id: node.id,
    storage_format: "plain",
    content: "# Large note",
    content_sha256: "sha",
    byte_len: 300_000,
    line_count: 2_000,
    start_line: 0,
    end_line: 999,
    returned_lines: 1_000,
    truncated: true,
    next_start_line: 1_000,
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_at: "2026-06-13T00:00:00Z"
  }
};

function renderTextEditorView() {
  render(
    <TextEditorView
      active
      node={node}
      mode="preview"
      canClose={false}
      onClose={vi.fn()}
      onSetMode={vi.fn()}
      onRenameNode={vi.fn()}
      onMoveNode={vi.fn()}
      onDeleteNode={vi.fn()}
    />
  );
}

describe("TextEditorView", () => {
  beforeEach(() => {
    vi.mocked(useTextDocument).mockReturnValue({
      data: partialText,
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useSaveTextDocument).mockReturnValue({ mutate: vi.fn(), isPending: false } as never);
  });

  it("disables editing for truncated text reads", () => {
    renderTextEditorView();

    expect(screen.getByText(/Loaded 1000 of 2000 lines/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
  });
});
