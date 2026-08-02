import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { getNode, resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { RestNode, Space } from "../../api/types";
import { useUiStore } from "../../stores/uiStore";
import { makeRestNode, makeSpace } from "../../test/fixtures";
import { createTestQueryClient } from "../../test/queryClient";
import {
  type CanonicalNodeLoader,
  useCanonicalNodeLoader
} from "./useCanonicalNodeLoader";
import { useWorkbenchNodeNavigationActions } from "./useWorkbenchNodeNavigationActions";

const mocks = vi.hoisted(() => ({
  revealNode: vi.fn()
}));

vi.mock("../../api/ApiProvider", () => ({
  useApiClient: () => ({})
}));

vi.mock("../../api/nodes", () => ({
  getNode: vi.fn(),
  resolveNodePath: vi.fn()
}));

vi.mock("./useWorkbenchQueries", () => ({
  useRevealNode: () => mocks.revealNode
}));

describe("useWorkbenchNodeNavigationActions", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUiStore.setState(useUiStore.getInitialState(), true);
    vi.mocked(getNode).mockReset();
    vi.mocked(resolveNodePath).mockReset();
    mocks.revealNode.mockReset();
  });

  it("opens a resolved markdown link through the active editor group and reveals its ancestors", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const folder = node("folder", activeSpace.id, "/Policies", "folder");
    const targetNode = node("target", activeSpace.id, "/Policies/Access Control Policy.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);
    mocks.revealNode.mockResolvedValue({ ancestors: [folder], target: targetNode });

    const { result, queryClient } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(resolveNodePath).toHaveBeenCalledWith(expect.anything(), activeSpace.id, targetNode.path);
    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().expandedFolderIds.has(folder.id)).toBe(true);
    expect(queryClient.getQueryData(queryKeys.node(activeSpace.id, targetNode.id))).toEqual(targetNode);
  });

  it("keeps the current editor state when markdown link resolution fails", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockRejectedValue(new ApiError("not found", 404));

    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, "/missing.md");
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Link target not found");
  });

  it("opens markdown links even when tree reveal fails", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const targetNode = node("target", activeSpace.id, "/Policies/Access Control Policy.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);
    mocks.revealNode.mockRejectedValue(new Error("reveal failed"));

    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().toast).toBe("Opened item, but could not reveal it in Files");
  });

  it("opens a resolved markdown link in the original source group when focus changes before resolution", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const otherNode = node("other", activeSpace.id, "/other.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    useUiStore.getState().openInNewGroup(otherNode);
    useUiStore.getState().focusGroup(0);
    const pending = deferred<RestNode>();
    vi.mocked(resolveNodePath).mockReturnValue(pending.promise);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: targetNode });

    const { result } = renderNavigationActions(activeSpace);

    const openPromise = result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    act(() => {
      useUiStore.getState().focusGroup(1);
      pending.resolve(targetNode);
    });
    await act(async () => {
      await openPromise;
    });

    const state = useUiStore.getState();
    expect(state.editorGroups[0].node?.id).toBe(targetNode.id);
    expect(state.editorGroups[1].node?.id).toBe(otherNode.id);
    expect(state.activeGroupIndex).toBe(1);
  });

  it("does not open a stale markdown link when the source group changed before resolution", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const replacementNode = node("replacement", activeSpace.id, "/replacement.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    const pending = deferred<RestNode>();
    vi.mocked(resolveNodePath).mockReturnValue(pending.promise);

    const { result } = renderNavigationActions(activeSpace);

    const openPromise = result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    act(() => {
      useUiStore.getState().openInGroup(groupId, replacementNode);
      pending.resolve(targetNode);
    });
    await act(async () => {
      await openPromise;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(replacementNode.id);
    expect(mocks.revealNode).not.toHaveBeenCalled();
  });

  it("does not open resolved markdown links from a different space", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/index.md");
    const targetNode = node("target", "space-2", "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    vi.mocked(resolveNodePath).mockResolvedValue(targetNode);

    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openMarkdownLink(groupId, sourceNode, targetNode.path);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Could not open link target");
  });

  it("opens regular nodes even when tree reveal fails", async () => {
    const activeSpace = space("space-1");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const loadCanonicalNode = vi.fn<CanonicalNodeLoader>().mockResolvedValue(targetNode);
    mocks.revealNode.mockRejectedValue(new Error("reveal failed"));

    const { result } = renderNavigationActions(activeSpace, loadCanonicalNode);

    await act(async () => {
      await result.current.openNode(targetNode);
    });

    expect(loadCanonicalNode).toHaveBeenCalledWith(targetNode, "Could not open item");
    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(targetNode.id);
    expect(useUiStore.getState().toast).toBe("Opened item, but could not reveal it in Files");
  });

  it("opens a regular node from reveal and seeds the canonical cache without a node request", async () => {
    const activeSpace = space("space-1");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    const targetNode = node("target", activeSpace.id, "/target.md");
    mocks.revealNode.mockResolvedValue({ ancestors: [folder], target: targetNode });
    const { result, queryClient } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    await act(async () => {
      await result.current.openNode(targetNode);
    });

    expect(mocks.revealNode).toHaveBeenCalledWith(activeSpace.id, targetNode.id);
    expect(getNode).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(queryKeys.node(activeSpace.id, targetNode.id))).toEqual(targetNode);
    expect(useUiStore.getState().editorGroups[0].node).toEqual(targetNode);
    expect(useUiStore.getState().expandedFolderIds.has(folder.id)).toBe(true);
  });

  it("opens an indexed link by node id and records it in editor navigation", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    mocks.revealNode
      .mockResolvedValueOnce({ ancestors: [], target: targetNode })
      .mockResolvedValueOnce({ ancestors: [], target: sourceNode });
    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openLinkedNode(activeSpace.id, targetNode.id);
    });

    let group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(targetNode.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([sourceNode.id]);

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(sourceNode.id);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual([targetNode.id]);
  });

  it("opens an indexed link in the original group when focus changes before resolution", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const otherNode = node("other", activeSpace.id, "/other.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    useUiStore.getState().openInNewGroup(otherNode);
    useUiStore.getState().focusGroup(0);
    const pending = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result } = renderNavigationActions(activeSpace);

    const openPromise = result.current.openLinkedNode(activeSpace.id, targetNode.id);
    act(() => {
      useUiStore.getState().focusGroup(1);
      pending.resolve({ ancestors: [], target: targetNode });
    });
    await act(async () => {
      await openPromise;
    });

    const state = useUiStore.getState();
    expect(state.editorGroups.find((group) => group.id === groupId)?.node?.id).toBe(targetNode.id);
    expect(state.editorGroups[1].node?.id).toBe(otherNode.id);
    expect(state.activeGroupIndex).toBe(1);
  });

  it("does not open a stale indexed link when its original group changes before resolution", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const replacementNode = node("replacement", activeSpace.id, "/replacement.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    const pending = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result } = renderNavigationActions(activeSpace);

    const openPromise = result.current.openLinkedNode(activeSpace.id, targetNode.id);
    act(() => {
      useUiStore.getState().openInGroup(groupId, replacementNode);
      pending.resolve({ ancestors: [folder], target: targetNode });
    });
    await act(async () => {
      await openPromise;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(replacementNode.id);
    expect(useUiStore.getState().expandedFolderIds.has(folder.id)).toBe(false);
  });

  it("opens a revealed node in a new editor group without a node request", async () => {
    const activeSpace = space("space-1");
    const current = node("current", activeSpace.id, "/current.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    openSourceGroup(activeSpace, current);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: targetNode });
    const { result } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    await act(async () => {
      await result.current.openNodeInNewGroup(targetNode);
    });

    expect(getNode).not.toHaveBeenCalled();
    expect(useUiStore.getState().editorGroups.map((group) => group.node?.id)).toEqual([
      current.id,
      targetNode.id
    ]);
  });

  it("opens a root node through the canonical loader without trying reveal", async () => {
    const activeSpace = space("space-1");
    const rootNode = node("root", activeSpace.id, "/", "folder", null);
    const loadCanonicalNode = vi.fn<CanonicalNodeLoader>().mockResolvedValue(rootNode);
    const { result } = renderNavigationActions(activeSpace, loadCanonicalNode);

    await act(async () => {
      await result.current.openNode(rootNode);
    });

    expect(mocks.revealNode).not.toHaveBeenCalled();
    expect(loadCanonicalNode).toHaveBeenCalledWith(rootNode, "Could not open item");
    expect(useUiStore.getState().editorGroups[0].node).toEqual(rootNode);
  });

  it("navigates backward from reveal without a node request", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const second = node("second", activeSpace.id, "/second.md");
    const third = node("third", activeSpace.id, "/third.md");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, second);
    useUiStore.getState().openInGroup(groupId, third);
    mocks.revealNode.mockResolvedValue({ ancestors: [folder], target: second });
    const { result, queryClient } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(mocks.revealNode).toHaveBeenCalledWith(activeSpace.id, second.id);
    expect(getNode).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(queryKeys.node(activeSpace.id, second.id))).toEqual(second);
    expect(useUiStore.getState().expandedFolderIds.has(folder.id)).toBe(true);
    expect(group.node?.id).toBe(second.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id]);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual([third.id]);
  });

  it("skips deleted navigation entries and continues in the same direction", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const deleted = node("deleted", activeSpace.id, "/deleted.md");
    const current = node("current", activeSpace.id, "/current.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, deleted);
    useUiStore.getState().openInGroup(groupId, current);
    mocks.revealNode
      .mockRejectedValueOnce(new ApiError("not found", 404))
      .mockResolvedValueOnce({ ancestors: [], target: first });
    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(first.id);
    expect(group.back).toEqual([]);
    expect(group.forward.map((entry) => entry.nodeId)).toEqual([current.id]);
    expect(mocks.revealNode).toHaveBeenCalledTimes(2);
    expect(getNode).not.toHaveBeenCalled();
  });

  it("falls back to a fresh node request when history reveal fails", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const current = node("current", activeSpace.id, "/current.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, current);
    mocks.revealNode.mockRejectedValue(new ApiError("unavailable", 503));
    vi.mocked(getNode).mockResolvedValue(first);
    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    expect(getNode).toHaveBeenCalledWith(expect.anything(), activeSpace.id, first.id);
    expect(useUiStore.getState().editorGroups[0].node).toEqual(first);
    expect(useUiStore.getState().toast).toBe("Opened item, but could not reveal it in Files");
  });

  it("keeps navigation history when the target cannot be verified", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const current = node("current", activeSpace.id, "/current.md");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, current);
    mocks.revealNode.mockRejectedValue(new ApiError("unavailable", 503));
    vi.mocked(getNode).mockRejectedValue(new ApiError("unavailable", 503));
    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.navigateEditorGroup(groupId, "back");
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(group.node?.id).toBe(current.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([first.id]);
    expect(useUiStore.getState().toast).toBe("Could not go back");
  });

  it("does not apply a navigation result after the group changes", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const current = node("current", activeSpace.id, "/current.md");
    const replacement = node("replacement", activeSpace.id, "/replacement.md");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    const groupId = openSourceGroup(activeSpace, first);
    useUiStore.getState().openInGroup(groupId, current);
    const pending = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result, queryClient } = renderNavigationActions(activeSpace);

    const navigation = result.current.navigateEditorGroup(groupId, "back");
    act(() => {
      useUiStore.getState().openInGroup(groupId, replacement);
      pending.resolve({ ancestors: [folder], target: first });
    });
    await act(async () => {
      await navigation;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(replacement.id);
    expect(queryClient.getQueryData(queryKeys.node(activeSpace.id, first.id))).toEqual(first);
    expect(useUiStore.getState().expandedFolderIds.has(folder.id)).toBe(false);
    expect(getNode).not.toHaveBeenCalled();
  });
});

function renderNavigationActions(
  activeSpace: Space | null,
  loadCanonicalNode: CanonicalNodeLoader = vi.fn()
) {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const rendered = renderHook(
    () => useWorkbenchNodeNavigationActions({ activeSpace, loadCanonicalNode }),
    { wrapper }
  );
  return { ...rendered, queryClient };
}

function renderNavigationActionsWithCanonicalLoader(activeSpace: Space) {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const rendered = renderHook(() => {
    const loadCanonicalNode = useCanonicalNodeLoader();
    return useWorkbenchNodeNavigationActions({ activeSpace, loadCanonicalNode });
  }, { wrapper });
  return { ...rendered, queryClient };
}

function openSourceGroup(activeSpace: Space, sourceNode: RestNode): number {
  useUiStore.getState().setActiveSpaceId(activeSpace.id);
  useUiStore.getState().openInActiveGroup(sourceNode);
  return useUiStore.getState().editorGroups[0].id;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function space(id: string): Space {
  return makeSpace({
    id,
    name: id,
    root_node_id: `${id}-root`
  });
}

function node(
  id: string,
  spaceId: string,
  path: string,
  kind: RestNode["kind"] = "text",
  parentId: string | null = `${spaceId}-root`
): RestNode {
  return makeRestNode({
    id,
    space_id: spaceId,
    parent_id: parentId,
    name: path.split("/").pop() ?? id,
    kind,
    path,
    has_children: kind === "folder"
  });
}
