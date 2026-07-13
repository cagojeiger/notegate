import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ReadTextResponse, RestNode } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import { useUiStore } from "../../stores/uiStore";
import { TextEditorView } from "./TextEditorView";
import { useMarkdownImageLoader, useSaveTextDocument, useTextDocument } from "./useEditorQueries";

vi.mock("../../shared/lib/clipboard", () => ({
  copyText: vi.fn()
}));

vi.mock("./useEditorQueries", () => ({
  useTextDocument: vi.fn(),
  useSaveTextDocument: vi.fn(),
  useMarkdownImageLoader: vi.fn()
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
    line_count: 5_001,
    start_line: 1,
    end_line: 5_000,
    returned_lines: 5_000,
    truncated: true,
    next_start_line: 5_001,
    updated_by: { id: "user-1", kind: "user", display_name: "User" },
    updated_at: "2026-06-13T00:00:00Z"
  }
};

function renderTextEditorView(canWriteActiveSpace = true) {
  render(
    <TextEditorView
      active
      groupId={0}
      node={node}
      mode="preview"
      canWriteActiveSpace={canWriteActiveSpace}
      canOpenInNewGroup
      canClose={false}
      onClose={vi.fn()}
      onSetMode={vi.fn()}
      onOpenNodeInNewGroup={vi.fn()}
      onOpenMarkdownLink={vi.fn()}
      onRenameNode={vi.fn()}
      onMoveNode={vi.fn()}
      onDeleteNode={vi.fn()}
    />
  );
}

describe("TextEditorView", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(useTextDocument).mockReturnValue({
      data: partialText,
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useSaveTextDocument).mockReturnValue({ mutate: vi.fn(), isPending: false } as never);
    vi.mocked(useMarkdownImageLoader).mockReset();
    vi.mocked(useMarkdownImageLoader).mockReturnValue(vi.fn().mockResolvedValue({ status: "error" }));
    vi.mocked(copyText).mockReset();
    vi.mocked(copyText).mockResolvedValue(true);
  });

  it("disables editing for truncated text reads", () => {
    renderTextEditorView();

    expect(screen.getByText(/Loaded 5000 of 5001 lines/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy content" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
  });

  it("disables editing without write permission", () => {
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

    renderTextEditorView(false);

    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
  });

  it("keeps encrypted text read-only", () => {
    vi.mocked(useTextDocument).mockReturnValue({
      data: {
        node: partialText.node,
        text: {
          node_id: node.id,
          storage_format: "encrypted",
          encrypted_payload: { ciphertext: "encrypted" },
          content_sha256: "sha",
          byte_len: 9,
          line_count: 1,
          updated_by: { id: "user-1", kind: "user", display_name: "User" },
          updated_at: "2026-06-13T00:00:00Z"
        }
      },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

    renderTextEditorView();

    expect(screen.getByText("Encrypted text cannot be previewed by the server.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy content" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
  });

  it("warns instead of overwriting a dirty editor when the server sha changes", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "original", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    const props = {
      active: true,
      groupId: 0,
      node,
      mode: "edit" as const,
      canWriteActiveSpace: true,
      canOpenInNewGroup: true,
      canClose: false,
      onClose: vi.fn(),
      onSetMode,
      onOpenNodeInNewGroup: vi.fn(),
      onOpenMarkdownLink: vi.fn(),
      onRenameNode: vi.fn(),
      onMoveNode: vi.fn(),
      onDeleteNode: vi.fn()
    };
    const view = render(<TextEditorView {...props} />);

    const textarea = screen.getByRole("textbox", { name: /edit text content/i });
    await waitFor(() => expect(textarea).toHaveValue("original"));
    await user.type(textarea, " local");
    view.rerender(<TextEditorView {...props} latestNode={{ ...node, content_sha256: "server-sha" }} />);

    expect(screen.getByText("This document changed outside this editor.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reload latest" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Keep editing" })).toBeInTheDocument();
  });

  it("resets horizontal edit scroll when the editor grows wider", () => {
    const originalResizeObserver = globalThis.ResizeObserver;
    const originalClientWidth = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "clientWidth");
    let triggerResize: (() => void) | null = null;
    let textareaWidth = 320;
    globalThis.ResizeObserver = class {
      constructor(callback: ResizeObserverCallback) {
        triggerResize = () => callback([], this as unknown as ResizeObserver);
      }
      observe() {}
      disconnect() {}
      unobserve() {}
    } as typeof ResizeObserver;
    Object.defineProperty(HTMLTextAreaElement.prototype, "clientWidth", {
      configurable: true,
      get: () => textareaWidth
    });

    try {
      vi.mocked(useTextDocument).mockReturnValue({
        data: { ...partialText, text: { ...partialText.text, content: "long line without wrapping", truncated: false, next_start_line: null } },
        isLoading: false,
        isError: false,
        isSuccess: true,
        refetch: vi.fn()
      } as never);
      render(
        <TextEditorView
          active
          groupId={0}
          node={node}
          mode="edit"
          canWriteActiveSpace
          canOpenInNewGroup
          canClose={false}
          onClose={vi.fn()}
          onSetMode={vi.fn()}
          onOpenNodeInNewGroup={vi.fn()}
          onOpenMarkdownLink={vi.fn()}
          onRenameNode={vi.fn()}
          onMoveNode={vi.fn()}
          onDeleteNode={vi.fn()}
        />
      );

      const textarea = screen.getByRole("textbox", { name: /edit text content/i });

      textarea.scrollLeft = 120;
      textareaWidth = 240;
      act(() => triggerResize?.());
      expect(textarea.scrollLeft).toBe(120);

      textareaWidth = 480;
      act(() => triggerResize?.());
      expect(textarea.scrollLeft).toBe(0);
    } finally {
      globalThis.ResizeObserver = originalResizeObserver;
      if (originalClientWidth) Object.defineProperty(HTMLTextAreaElement.prototype, "clientWidth", originalClientWidth);
      else delete (HTMLTextAreaElement.prototype as unknown as { clientWidth?: number }).clientWidth;
    }
  });

  it("copies loaded text from the editor header", async () => {
    const user = userEvent.setup();
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "copy me", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

    renderTextEditorView();

    await user.click(screen.getByRole("button", { name: "Copy content" }));

    expect(copyText).toHaveBeenCalledWith("copy me");
  });

  it("uses save and cancel actions while editing text", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    const save = vi.fn();
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "original", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useSaveTextDocument).mockReturnValue({ mutate: save, isPending: false } as never);
    const props = {
      active: true,
      groupId: 0,
      node,
      canWriteActiveSpace: true,
      canOpenInNewGroup: true,
      canClose: false,
      onClose: vi.fn(),
      onSetMode,
      onOpenNodeInNewGroup: vi.fn(),
      onOpenMarkdownLink: vi.fn(),
      onRenameNode: vi.fn(),
      onMoveNode: vi.fn(),
      onDeleteNode: vi.fn()
    };
    const view = render(<TextEditorView {...props} mode="preview" />);

    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(onSetMode).toHaveBeenCalledWith("edit");

    view.rerender(<TextEditorView {...props} mode="edit" />);
    await waitFor(() => expect(screen.getByRole("textbox", { name: /edit text content/i })).toHaveValue("original"));
    expect(screen.queryByRole("button", { name: "Preview" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel edit" })).toBeInTheDocument();

    await user.type(screen.getByRole("textbox", { name: /edit text content/i }), " changed");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith(false);

    await user.click(screen.getByRole("button", { name: "Cancel edit" }));
    expect(onSetMode).toHaveBeenLastCalledWith("preview");
    expect(useUiStore.getState().toast).toBe("Edit canceled");
  });

  it("loads the draft when restored directly into edit mode", async () => {
    const user = userEvent.setup();
    const save = vi.fn();
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "restored content", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useSaveTextDocument).mockReturnValue({ mutate: save, isPending: false } as never);

    render(
      <TextEditorView
        active
        groupId={0}
        node={node}
        mode="edit"
        canWriteActiveSpace
        canOpenInNewGroup
        canClose={false}
        onClose={vi.fn()}
        onSetMode={vi.fn()}
        onOpenNodeInNewGroup={vi.fn()}
        onOpenMarkdownLink={vi.fn()}
        onRenameNode={vi.fn()}
        onMoveNode={vi.fn()}
        onDeleteNode={vi.fn()}
      />
    );

    const textarea = screen.getByRole("textbox", { name: /edit text content/i });
    await waitFor(() => expect(textarea).toHaveValue("restored content"));
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    await user.type(textarea, " changed");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(save).toHaveBeenCalledWith(false);
  });

  it("passes the source group and node when opening markdown links", async () => {
    const onOpenMarkdownLink = vi.fn();
    const sourceNode = { ...node, path: "/docs/source.md" };
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "[Target](./target.md)", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

    render(
      <TextEditorView
        active
        groupId={7}
        node={sourceNode}
        mode="preview"
        canWriteActiveSpace
        canOpenInNewGroup
        canClose={false}
        onClose={vi.fn()}
        onSetMode={vi.fn()}
        onOpenNodeInNewGroup={vi.fn()}
        onOpenMarkdownLink={onOpenMarkdownLink}
        onRenameNode={vi.fn()}
        onMoveNode={vi.fn()}
        onDeleteNode={vi.fn()}
      />
    );

    fireEvent.click(await screen.findByRole("link", { name: "Target" }));

    expect(onOpenMarkdownLink).toHaveBeenCalledWith(7, expect.objectContaining({ id: sourceNode.id, path: sourceNode.path }), "/docs/target.md");
  });

  it("shows a toast for invalid internal-looking markdown links", async () => {
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "[Broken](./bad%path.md)", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

    renderTextEditorView();

    fireEvent.click(await screen.findByRole("link", { name: "Broken" }));

    expect(useUiStore.getState().toast).toBe("Invalid markdown link");
  });

  it("passes markdown image links through the editor image loader", async () => {
    const objectUrls = installObjectUrlMock();
    const loadMarkdownImage = vi.fn().mockResolvedValue({ status: "loaded", blob: new Blob(["image"], { type: "image/png" }) });
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "![Diagram](./assets/diagram.png)", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useMarkdownImageLoader).mockReturnValue(loadMarkdownImage);
    const sourceNode = { ...node, path: "/docs/source.md" };
    const viewProps = {
      active: true,
      groupId: 0,
      node: sourceNode,
      mode: "preview" as const,
      canWriteActiveSpace: true,
      canOpenInNewGroup: true,
      canClose: false,
      onClose: vi.fn(),
      onSetMode: vi.fn(),
      onOpenNodeInNewGroup: vi.fn(),
      onOpenMarkdownLink: vi.fn(),
      onRenameNode: vi.fn(),
      onMoveNode: vi.fn(),
      onDeleteNode: vi.fn()
    };

    try {
      const view = render(<TextEditorView {...viewProps} />);

      expect(await screen.findByRole("img", { name: "Diagram" })).toHaveAttribute("src", "blob:notegate-editor-test");
      expect(useMarkdownImageLoader).toHaveBeenCalledWith(expect.objectContaining({ id: sourceNode.id, path: sourceNode.path }));
      expect(loadMarkdownImage).toHaveBeenCalledWith("/docs/assets/diagram.png");
      expect(loadMarkdownImage).toHaveBeenCalledTimes(1);

      view.rerender(<TextEditorView {...viewProps} />);
      await waitFor(() => expect(loadMarkdownImage).toHaveBeenCalledTimes(1));
      view.unmount();
    } finally {
      objectUrls.restore();
    }
  });

  it("shows a placeholder for unsupported markdown images", async () => {
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "![Not image](./note.md)", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useMarkdownImageLoader).mockReturnValue(vi.fn().mockResolvedValue({ status: "unsupported" }));

    renderTextEditorView();

    expect(await screen.findByText("Image cannot be displayed: Not image")).toBeInTheDocument();
  });

  it("shows editor actions from the preview context menu", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    const onOpenNodeInNewGroup = vi.fn();
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "plain text", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);

    render(
      <TextEditorView
        active
        groupId={0}
        node={{ ...node, name: "note.txt" }}
        mode="preview"
        canWriteActiveSpace
        canOpenInNewGroup
        canClose
        onClose={vi.fn()}
        onSetMode={onSetMode}
        onOpenNodeInNewGroup={onOpenNodeInNewGroup}
        onOpenMarkdownLink={vi.fn()}
        onRenameNode={vi.fn()}
        onMoveNode={vi.fn()}
        onDeleteNode={vi.fn()}
      />
    );

    fireEvent.contextMenu(screen.getByText("plain text"));

    await user.click(within(screen.getByRole("menu")).getByRole("button", { name: "Copy content" }));
    expect(copyText).toHaveBeenCalledWith("plain text");

    fireEvent.contextMenu(screen.getByText("plain text"));
    await user.click(within(screen.getByRole("menu")).getByRole("button", { name: "Edit" }));
    expect(onSetMode).toHaveBeenCalledWith("edit");

    fireEvent.contextMenu(screen.getByText("plain text"));
    await user.click(within(screen.getByRole("menu")).getByRole("button", { name: "Open in new group" }));
    expect(onOpenNodeInNewGroup).toHaveBeenCalledWith(expect.objectContaining({ id: node.id }));
  });

  it("shows save and cancel actions from the edit context menu", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    const save = vi.fn();
    vi.mocked(useTextDocument).mockReturnValue({
      data: { ...partialText, text: { ...partialText.text, content: "original", truncated: false, next_start_line: null } },
      isLoading: false,
      isError: false,
      isSuccess: true,
      refetch: vi.fn()
    } as never);
    vi.mocked(useSaveTextDocument).mockReturnValue({ mutate: save, isPending: false } as never);
    const props = {
      active: true,
      groupId: 0,
      node,
      canWriteActiveSpace: true,
      canOpenInNewGroup: true,
      canClose: false,
      onClose: vi.fn(),
      onSetMode,
      onOpenNodeInNewGroup: vi.fn(),
      onOpenMarkdownLink: vi.fn(),
      onRenameNode: vi.fn(),
      onMoveNode: vi.fn(),
      onDeleteNode: vi.fn()
    };
    render(<TextEditorView {...props} mode="edit" />);
    await waitFor(() => expect(screen.getByRole("textbox", { name: /edit text content/i })).toHaveValue("original"));
    const textarea = screen.getByRole("textbox", { name: /edit text content/i });
    await user.type(textarea, " changed");

    fireEvent.contextMenu(textarea);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    fireEvent.contextMenu(textarea.parentElement ?? textarea);
    await user.click(within(screen.getByRole("menu")).getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith(false);

    fireEvent.contextMenu(textarea.parentElement ?? textarea);
    await user.click(within(screen.getByRole("menu")).getByRole("button", { name: "Cancel edit" }));
    expect(onSetMode).toHaveBeenLastCalledWith("preview");
    expect(useUiStore.getState().toast).toBe("Edit canceled");
  });

});

function installObjectUrlMock() {
  const originalCreateObjectURL = URL.createObjectURL;
  const originalRevokeObjectURL = URL.revokeObjectURL;
  const createObjectURL = vi.fn().mockReturnValue("blob:notegate-editor-test");
  const revokeObjectURL = vi.fn();

  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURL });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectURL });

  return {
    createObjectURL,
    revokeObjectURL,
    restore: () => {
      if (originalCreateObjectURL) Object.defineProperty(URL, "createObjectURL", { configurable: true, value: originalCreateObjectURL });
      else delete (URL as unknown as { createObjectURL?: unknown }).createObjectURL;
      if (originalRevokeObjectURL) Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: originalRevokeObjectURL });
      else delete (URL as unknown as { revokeObjectURL?: unknown }).revokeObjectURL;
    }
  };
}
