import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ReadTextResponse } from "../../api/types";
import { copyText } from "../../shared/lib/clipboard";
import { useUiStore } from "../../stores/uiStore";
import { installElementResizeMock } from "../../test/browserLayout";
import { makeRestNode } from "../../test/fixtures";
import { TextEditorView } from "./TextEditorView";
import type { useSaveTextDocument, useTextDocument } from "./useEditorQueries";
import { useMarkdownImageLoader } from "./useFilePreviewQueries";

type TextEditorViewProps = Parameters<typeof TextEditorView>[0];
type TextDocumentQuery = ReturnType<typeof useTextDocument>;
type TextDocumentQueryMock = Pick<
  TextDocumentQuery,
  "data" | "isError" | "isLoading" | "isSuccess" | "refetch"
>;
type SaveTextMutation = ReturnType<typeof useSaveTextDocument>;
type SaveTextMutationMock = Pick<SaveTextMutation, "mutate" | "isPending">;

const editorQueryMocks = vi.hoisted(() => ({
  useTextDocument: vi.fn<(...args: Parameters<typeof useTextDocument>) => TextDocumentQueryMock>(),
  useSaveTextDocument: vi.fn<(...args: Parameters<typeof useSaveTextDocument>) => SaveTextMutationMock>()
}));

const markdownRenderMocks = vi.hoisted(() => ({
  reactMarkdown: vi.fn()
}));

vi.mock("react-markdown", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-markdown")>();

  return {
    ...actual,
    default: (props: ComponentProps<typeof actual.default>) => {
      markdownRenderMocks.reactMarkdown();
      const Actual = actual.default;
      return <Actual {...props} />;
    }
  };
});

vi.mock("../../shared/lib/clipboard", () => ({
  copyText: vi.fn()
}));

vi.mock("./useEditorQueries", () => ({
  useTextDocument: editorQueryMocks.useTextDocument,
  useSaveTextDocument: editorQueryMocks.useSaveTextDocument
}));

vi.mock("./useFilePreviewQueries", () => ({
  useMarkdownImageLoader: vi.fn()
}));

const node = makeRestNode({
  name: "large.md",
  path: "/large.md",
});

const partialText = {
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
} satisfies ReadTextResponse;

function mockTextDocument(data: ReadTextResponse = partialText) {
  const query = {
    data,
    isError: false,
    isLoading: false,
    isSuccess: true,
    refetch: vi.fn()
  } satisfies TextDocumentQueryMock;
  editorQueryMocks.useTextDocument.mockReturnValue(query);
}

function mockFullText(content = partialText.text.content) {
  mockTextDocument({
    ...partialText,
    text: { ...partialText.text, content, truncated: false, next_start_line: null }
  });
}

function mockSaveTextDocument(mutate: SaveTextMutation["mutate"] = vi.fn()) {
  const mutation = {
    mutate,
    isPending: false
  } satisfies SaveTextMutationMock;
  editorQueryMocks.useSaveTextDocument.mockReturnValue(mutation);
}

function makeTextEditorViewProps(overrides: Partial<TextEditorViewProps> = {}): TextEditorViewProps {
  return {
    active: true,
    groupId: 0,
    node,
    qualifiedPath: "Daily:/large.md",
    mode: "preview",
    canWriteActiveSpace: true,
    canOpenInNewGroup: true,
    canClose: false,
    onClose: vi.fn(),
    onSetMode: vi.fn(),
    onOpenNodeInNewGroup: vi.fn(),
    onOpenMarkdownLink: vi.fn(),
    onRenameNode: vi.fn(),
    onMoveNode: vi.fn(),
    onDeleteNode: vi.fn(),
    ...overrides
  };
}

function renderTextEditorView(overrides: Partial<TextEditorViewProps> = {}) {
  return render(<TextEditorView {...makeTextEditorViewProps(overrides)} />);
}

describe("TextEditorView", () => {
  beforeEach(() => {
    useUiStore.setState(useUiStore.getInitialState(), true);
    mockTextDocument();
    mockSaveTextDocument();
    vi.mocked(useMarkdownImageLoader).mockReset();
    vi.mocked(useMarkdownImageLoader).mockReturnValue(vi.fn().mockResolvedValue({ status: "error" }));
    vi.mocked(copyText).mockReset();
    vi.mocked(copyText).mockResolvedValue(true);
    markdownRenderMocks.reactMarkdown.mockReset();
  });

  it("disables editing for truncated text reads", () => {
    renderTextEditorView();

    expect(screen.getByText(/Loaded 5000 of 5001 lines/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy content" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
  });

  it("switches CSV previews between table and exact source", async () => {
    const user = userEvent.setup();
    const csvNode = { ...node, name: "people.csv", path: "/people.csv" };
    mockFullText("name,role\nAda,engineer");

    renderTextEditorView({ node: csvNode });

    const tableButton = screen.getByRole("button", { name: "Table" });
    const sourceButton = screen.getByRole("button", { name: "Source" });
    expect(tableButton).toHaveAttribute("aria-pressed", "true");
    expect(sourceButton).toHaveAttribute("aria-pressed", "false");
    expect(await screen.findByRole("table", { name: "CSV data" })).toBeInTheDocument();

    await user.click(sourceButton);

    expect(sourceButton).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("region", { name: "CSV source" }).querySelector("pre")?.textContent).toBe("name,role\nAda,engineer");
    expect(screen.queryByRole("table", { name: "CSV data" })).not.toBeInTheDocument();
  });

  it("keeps truncated CSV previews in source mode", async () => {
    const csvNode = { ...node, name: "people.csv", path: "/people.csv" };
    mockTextDocument({
      ...partialText,
      text: { ...partialText.text, content: "name,role\nAda,engineer" }
    });

    renderTextEditorView({ node: csvNode });

    expect(screen.getByRole("button", { name: "Table" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute("aria-pressed", "true");
    expect((await screen.findByRole("region", { name: "CSV source" })).querySelector("pre")?.textContent).toBe("name,role\nAda,engineer");
  });

  it("disables editing without write permission", () => {
    mockFullText();

    renderTextEditorView({ canWriteActiveSpace: false });

    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Copy path" })).toBeEnabled();
  });

  it("copies the qualified path from the header", async () => {
    const user = userEvent.setup();
    mockFullText();
    renderTextEditorView({ qualifiedPath: "daily:/research/review.md", canWriteActiveSpace: false });

    await user.click(screen.getByRole("button", { name: "Copy path" }));

    expect(copyText).toHaveBeenCalledWith("daily:/research/review.md");
    expect(useUiStore.getState().toast).toBe("Path copied");
  });

  it("reports when the qualified path could not be copied", async () => {
    const user = userEvent.setup();
    mockFullText();
    vi.mocked(copyText).mockResolvedValue(false);
    renderTextEditorView();

    await user.click(screen.getByRole("button", { name: "Copy path" }));

    expect(useUiStore.getState().toast).toBe("Could not copy path");
  });

  it("keeps read actions available while disabling every write action under a lock", async () => {
    mockFullText();
    const lockedNode = { ...node, effective_write_locked: true };

    renderTextEditorView({ node: lockedNode });

    expect(screen.getByRole("button", { name: "Copy content" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
    const moreActions = screen.getByRole("button", { name: "More actions" });
    expect(moreActions).toBeEnabled();
    await userEvent.click(moreActions);
    const actionDialog = within(screen.getByRole("dialog", { name: "More actions" }));
    expect(actionDialog.getByRole("button", { name: "Copy content" })).toBeEnabled();
    expect(actionDialog.getByRole("button", { name: "Rename" })).toBeDisabled();
    await userEvent.click(actionDialog.getByRole("button", { name: "Copy content" }));

    fireEvent.contextMenu(await screen.findByText("Large note"));
    const menu = within(screen.getByRole("menu"));
    expect(menu.getByRole("button", { name: "Copy content" })).toBeEnabled();
    expect(menu.getByRole("button", { name: "Copy path" })).toBeEnabled();
    expect(menu.getByRole("button", { name: "Edit" })).toBeDisabled();
    expect(menu.getByRole("button", { name: "Rename" })).toBeDisabled();
    expect(menu.getByRole("button", { name: "Move…" })).toBeDisabled();
    expect(menu.getByRole("button", { name: "Delete" })).toBeDisabled();

    await userEvent.click(menu.getByRole("button", { name: "Copy path" }));
    expect(copyText).toHaveBeenCalledWith("Daily:/large.md");
  });

  it("keeps a dirty draft visible and read-only when a lock arrives", async () => {
    const user = userEvent.setup();
    const props = makeTextEditorViewProps({ mode: "edit" });
    mockFullText("original");
    const view = render(<TextEditorView {...props} />);
    const textarea = screen.getByRole("textbox", { name: /edit text content/i });
    await waitFor(() => expect(textarea).toHaveValue("original"));
    await user.type(textarea, " unsaved");

    view.rerender(<TextEditorView {...props} node={{ ...node, effective_write_locked: true }} />);

    expect(textarea).toHaveValue("original unsaved");
    expect(textarea).toHaveAttribute("readonly");
    expect(screen.getByText(/Unsaved edits are preserved/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    const copy = screen.getByRole("button", { name: "Copy content" });
    expect(copy).toBeEnabled();
    await user.click(copy);
    expect(copyText).toHaveBeenCalledWith("original unsaved");
  });

  it("keeps encrypted text read-only", () => {
    mockTextDocument({
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
    });

    renderTextEditorView();

    expect(screen.getByText("Encrypted text cannot be previewed by the server.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy content" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();
  });

  it("warns instead of overwriting a dirty editor when the server sha changes", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    mockFullText("original");
    const props = makeTextEditorViewProps({ mode: "edit", onSetMode });
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
    const resize = installElementResizeMock();

    try {
      mockFullText("long line without wrapping");
      renderTextEditorView({ mode: "edit" });

      const textarea = screen.getByRole("textbox", { name: /edit text content/i });

      textarea.scrollLeft = 120;
      resize.setWidth(textarea, 240);
      act(() => resize.trigger(textarea));
      expect(textarea.scrollLeft).toBe(120);

      resize.setWidth(textarea, 480);
      act(() => resize.trigger(textarea));
      expect(textarea.scrollLeft).toBe(0);
    } finally {
      resize.restore();
    }
  });

  it("copies loaded text from the editor header", async () => {
    const user = userEvent.setup();
    mockFullText("copy me");

    renderTextEditorView();

    await user.click(screen.getByRole("button", { name: "Copy content" }));

    expect(copyText).toHaveBeenCalledWith("copy me");
  });

  it("uses save and cancel actions while editing text", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    const save = vi.fn();
    mockFullText("original");
    mockSaveTextDocument(save);
    const props = makeTextEditorViewProps({ onSetMode });
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
    mockFullText("restored content");
    mockSaveTextDocument(save);

    renderTextEditorView({ mode: "edit" });

    const textarea = screen.getByRole("textbox", { name: /edit text content/i });
    await waitFor(() => expect(textarea).toHaveValue("restored content"));
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    await user.type(textarea, " changed");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(save).toHaveBeenCalledWith(false);
  });

  it("does not reparse markdown when only the link callback changes", async () => {
    const previousOpenMarkdownLink = vi.fn();
    const currentOpenMarkdownLink = vi.fn();
    const sourceNode = { ...node, path: "/docs/source.md" };
    mockFullText("[Target](./target.md)");
    const props = makeTextEditorViewProps({
      groupId: 7,
      node: sourceNode,
      onOpenMarkdownLink: previousOpenMarkdownLink
    });

    const view = render(<TextEditorView {...props} />);
    const link = await screen.findByRole("link", { name: "Target" });
    const renderCount = markdownRenderMocks.reactMarkdown.mock.calls.length;

    view.rerender(<TextEditorView {...props} onOpenMarkdownLink={currentOpenMarkdownLink} />);

    expect(markdownRenderMocks.reactMarkdown).toHaveBeenCalledTimes(renderCount);
    fireEvent.click(link);

    expect(previousOpenMarkdownLink).not.toHaveBeenCalled();
    expect(currentOpenMarkdownLink).toHaveBeenCalledWith(7, expect.objectContaining({ id: sourceNode.id, path: sourceNode.path }), "/docs/target.md");
  });

  it("shows a toast for invalid internal-looking markdown links", async () => {
    mockFullText("[Broken](./bad%path.md)");

    renderTextEditorView();

    fireEvent.click(await screen.findByRole("link", { name: "Broken" }));

    expect(useUiStore.getState().toast).toBe("Invalid markdown link");
  });

  it("passes markdown image links through the editor image loader", async () => {
    const loadMarkdownImage = vi.fn().mockResolvedValue({ status: "loaded", url: "https://storage.example/image.png" });
    mockFullText("![Diagram](./assets/diagram.png)");
    vi.mocked(useMarkdownImageLoader).mockReturnValue(loadMarkdownImage);
    const sourceNode = { ...node, path: "/docs/source.md" };
    const viewProps = makeTextEditorViewProps({ node: sourceNode });

    const view = render(<TextEditorView {...viewProps} />);

    expect(await screen.findByRole("img", { name: "Diagram" })).toHaveAttribute("src", "https://storage.example/image.png");
    expect(useMarkdownImageLoader).toHaveBeenCalledWith(expect.objectContaining({ id: sourceNode.id, path: sourceNode.path }));
    expect(loadMarkdownImage).toHaveBeenCalledWith("/docs/assets/diagram.png");
    expect(loadMarkdownImage).toHaveBeenCalledTimes(1);

    view.rerender(<TextEditorView {...viewProps} />);
    await waitFor(() => expect(loadMarkdownImage).toHaveBeenCalledTimes(1));
  });

  it("shows a placeholder for unsupported markdown images", async () => {
    mockFullText("![Not image](./note.md)");
    vi.mocked(useMarkdownImageLoader).mockReturnValue(vi.fn().mockResolvedValue({ status: "unsupported" }));

    renderTextEditorView();

    expect(await screen.findByText("Image cannot be displayed: Not image")).toBeInTheDocument();
  });

  it("shows a placeholder when the browser cannot decode a markdown image", async () => {
    mockFullText("![Broken](./broken.png)");
    const loadMarkdownImage = vi.fn().mockResolvedValue({ status: "loaded", url: "https://storage.example/broken.png" });
    vi.mocked(useMarkdownImageLoader).mockReturnValue(loadMarkdownImage);
    renderTextEditorView();

    fireEvent.error(await screen.findByRole("img", { name: "Broken" }));
    await waitFor(() => expect(loadMarkdownImage).toHaveBeenCalledTimes(2));
    expect(loadMarkdownImage).toHaveBeenLastCalledWith("/broken.png", { forceRefresh: true });
    fireEvent.error(screen.getByRole("img", { name: "Broken" }));

    expect(await screen.findByText("Could not load image: Broken")).toBeInTheDocument();
  });

  it("shows editor actions from the preview context menu", async () => {
    const user = userEvent.setup();
    const onSetMode = vi.fn();
    const onOpenNodeInNewGroup = vi.fn();
    mockFullText("plain text");

    renderTextEditorView({
      node: { ...node, name: "note.txt" },
      canClose: true,
      onSetMode,
      onOpenNodeInNewGroup
    });

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
    mockFullText("original");
    mockSaveTextDocument(save);
    renderTextEditorView({ mode: "edit", onSetMode });
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
