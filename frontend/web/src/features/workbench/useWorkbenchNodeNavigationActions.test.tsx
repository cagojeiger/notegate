import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/errors";
import { getNode, resolveNodePath } from "../../api/nodes";
import { queryKeys } from "../../api/queryKeys";
import type { NodeRevealResponse, RestNode, Space } from "../../api/types";
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

    expect(loadCanonicalNode).toHaveBeenCalledWith(targetNode, null);
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

  it("returns the current reveal path instead of the path in a stale summary", async () => {
    const activeSpace = space("space-1");
    const current = node("target", activeSpace.id, "/renamed/target.md");
    const stale = { ...current, path: "/old/target.md" };
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: current });
    const { result } = renderNavigationActions(activeSpace);

    const open = result.current.openNode(stale);
    await act(async () => {
      await open;
    });

    expect((await open)?.target.path).toBe(current.path);
    expect(useUiStore.getState().editorGroups[0].node?.path).toBe(current.path);
  });

  it("ignores an older open response after a newer node is opened", async () => {
    const activeSpace = space("space-1");
    const first = node("first", activeSpace.id, "/first.md");
    const second = node("second", activeSpace.id, "/second.md");
    const firstReveal = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    const secondReveal = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    mocks.revealNode.mockImplementation((_spaceId: string, nodeId: string) =>
      nodeId === first.id ? firstReveal.promise : secondReveal.promise
    );
    const { result } = renderNavigationActions(activeSpace);

    const firstOpen = result.current.openNode(first);
    const secondOpen = result.current.openNode(second);
    await act(async () => {
      secondReveal.resolve({ ancestors: [], target: second });
      await secondOpen;
    });
    await act(async () => {
      firstReveal.resolve({ ancestors: [], target: first });
      expect(await firstOpen).toBeNull();
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(second.id);
  });

  it.each([
    { action: "active", stage: "reveal", outcome: "success" },
    { action: "new", stage: "reveal", outcome: "success" },
    { action: "active", stage: "reveal", outcome: "failure" },
    { action: "active", stage: "fallback", outcome: "success" },
    { action: "new", stage: "fallback", outcome: "failure" }
  ] as const)("ignores a stale $action open $stage $outcome after switching spaces", async ({ action, stage, outcome }) => {
    const firstSpace = space("space-1");
    const nextSpace = space("space-2");
    const sourceNode = node("source", firstSpace.id, "/source.md");
    const targetNode = node("target", firstSpace.id, "/folder/target.md");
    const folder = node("folder", firstSpace.id, "/folder", "folder");
    const nextNode = node("next", nextSpace.id, "/next.md");
    openSourceGroup(firstSpace, sourceNode);
    const reveal = deferred<NodeRevealResponse>();
    const fallback = deferred<RestNode>();
    mocks.revealNode.mockReturnValue(reveal.promise);
    vi.mocked(getNode).mockReturnValue(fallback.promise);
    const { result, rerender, queryClient } = renderNavigationActionsWithCanonicalLoader(firstSpace);

    const opening = action === "new"
      ? result.current.openNodeInNewGroup(targetNode)
      : result.current.openNode(targetNode);
    if (stage === "fallback") {
      await act(async () => reveal.reject(new ApiError("unavailable", 503)));
      await waitFor(() => expect(getNode).toHaveBeenCalledWith(expect.anything(), firstSpace.id, targetNode.id));
    }
    act(() => {
      useUiStore.getState().setActiveSpaceId(nextSpace.id);
      useUiStore.getState().openInActiveGroup(nextNode);
      useUiStore.getState().setExpanded(["next-folder"]);
      useUiStore.getState().toggleMobileTree();
      useUiStore.getState().showToast("Current space notification");
    });
    rerender({ activeSpace: nextSpace, inspectedNode: nextNode });
    const currentUi = navigationUiState();

    await act(async () => {
      if (stage === "reveal") {
        if (outcome === "success") reveal.resolve({ ancestors: [folder], target: targetNode });
        else reveal.reject(new ApiError("unavailable", 503));
      } else {
        if (outcome === "success") fallback.resolve(targetNode);
        else fallback.reject(new ApiError("unavailable", 503));
      }
      await opening;
    });

    expect(navigationUiState()).toEqual(currentUi);
    if (stage === "reveal") expect(getNode).not.toHaveBeenCalled();
    if (outcome === "success") {
      expect(queryClient.getQueryData(queryKeys.node(firstSpace.id, targetNode.id))).toEqual(targetNode);
    }
  });

  it("does not revive a pending open after leaving and returning to the same space", async () => {
    const activeSpace = space("space-1");
    const otherSpace = space("space-2");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const targetNode = node("target", activeSpace.id, "/folder/target.md");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    openSourceGroup(activeSpace, sourceNode);
    const pending = deferred<NodeRevealResponse>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result, rerender } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    const opening = result.current.openNode(targetNode);
    act(() => useUiStore.getState().setActiveSpaceId(otherSpace.id));
    rerender({ activeSpace: otherSpace, inspectedNode: null });
    act(() => useUiStore.getState().setActiveSpaceId(activeSpace.id));
    rerender({ activeSpace, inspectedNode: currentActiveNode() });
    const restoredUi = navigationUiState();
    await act(async () => {
      pending.resolve({ ancestors: [folder], target: targetNode });
      expect(await opening).toBeNull();
    });

    expect(navigationUiState()).toEqual(restoredUi);
    expect(currentActiveNode()?.id).toBe(sourceNode.id);
  });

  it("rejects an old-space callback before requesting data or superseding the current open", async () => {
    const firstSpace = space("space-1");
    const nextSpace = space("space-2");
    const oldTarget = node("old-target", firstSpace.id, "/old-target.md");
    const nextTarget = node("next-target", nextSpace.id, "/next-target.md");
    const pending = deferred<NodeRevealResponse>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result, rerender } = renderNavigationActionsWithCanonicalLoader(firstSpace);
    const openFromPreviousSpace = result.current.openNode;

    act(() => useUiStore.getState().setActiveSpaceId(nextSpace.id));
    rerender({ activeSpace: nextSpace, inspectedNode: null });
    const opening = result.current.openNode(nextTarget);
    const currentUi = navigationUiState();
    await act(async () => {
      expect(await openFromPreviousSpace(oldTarget)).toBeNull();
    });

    expect(navigationUiState()).toEqual(currentUi);
    expect(mocks.revealNode).toHaveBeenCalledTimes(1);
    expect(mocks.revealNode).toHaveBeenCalledWith(nextSpace.id, nextTarget.id);
    expect(getNode).not.toHaveBeenCalled();
    await act(async () => {
      pending.resolve({ ancestors: [], target: nextTarget });
      expect((await opening)?.target).toEqual(nextTarget);
    });
    expect(currentActiveNode()).toEqual(nextTarget);
  });

  it.each([
    { action: "active", change: "focus" },
    { action: "active", change: "edit" },
    { action: "new", change: "close" },
    { action: "new", change: "replace" }
  ] as const)("ignores a pending $action open after the source group $change changes", async ({ action, change }) => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const otherNode = node("other", activeSpace.id, "/other.md");
    const targetNode = node("target", activeSpace.id, "/folder/target.md");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    const groupId = openSourceGroup(activeSpace, sourceNode);
    useUiStore.getState().openInNewGroup(otherNode);
    useUiStore.getState().focusGroup(0);
    const pending = deferred<NodeRevealResponse>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    const opening = action === "new"
      ? result.current.openNodeInNewGroup(targetNode)
      : result.current.openNode(targetNode);
    act(() => {
      if (change === "focus") useUiStore.getState().focusGroup(1);
      if (change === "edit") useUiStore.getState().setGroupMode(0, "edit");
      if (change === "close") useUiStore.getState().closeGroup(0);
      if (change === "replace") useUiStore.getState().openInGroup(groupId, otherNode);
      useUiStore.getState().toggleMobileAux();
    });
    const currentUi = navigationUiState();
    await act(async () => {
      pending.resolve({ ancestors: [folder], target: targetNode });
      await opening;
    });

    expect(navigationUiState()).toEqual(currentUi);
    expect(getNode).not.toHaveBeenCalled();
  });

  it.each([
    { action: "active", stage: "reveal" },
    { action: "new", stage: "fallback" }
  ] as const)("keeps reopened mobile panels open after a pending $action open completes through $stage", async ({ action, stage }) => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    openSourceGroup(activeSpace, sourceNode);
    const reveal = deferred<NodeRevealResponse>();
    const fallback = deferred<RestNode>();
    mocks.revealNode.mockReturnValue(reveal.promise);
    vi.mocked(getNode).mockReturnValue(fallback.promise);
    const { result } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    const opening = action === "new"
      ? result.current.openNodeInNewGroup(targetNode)
      : result.current.openNode(targetNode);
    if (stage === "fallback") {
      await act(async () => reveal.reject(new ApiError("unavailable", 503)));
      await waitFor(() => expect(getNode).toHaveBeenCalledWith(expect.anything(), activeSpace.id, targetNode.id));
    }
    act(() => {
      useUiStore.getState().toggleMobileTree();
      useUiStore.getState().toggleMobileAux();
    });
    await act(async () => {
      if (stage === "reveal") reveal.resolve({ ancestors: [], target: targetNode });
      else fallback.resolve(targetNode);
      await opening;
    });

    const state = useUiStore.getState();
    expect(state.editorGroups.map((group) => group.node?.id)).toEqual(
      action === "new" ? [sourceNode.id, targetNode.id] : [targetNode.id]
    );
    expect(currentActiveNode()).toEqual(targetNode);
    expect(state.mobileTreeOpen).toBe(true);
    expect(state.mobileAuxOpen).toBe(true);
    if (stage === "reveal") expect(getNode).not.toHaveBeenCalled();
  });

  it("allows two explicit new-group opens to complete after the first changes focus", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const first = node("first", activeSpace.id, "/first.md");
    const second = node("second", activeSpace.id, "/second.md");
    openSourceGroup(activeSpace, sourceNode);
    const firstReveal = deferred<NodeRevealResponse>();
    const secondReveal = deferred<NodeRevealResponse>();
    mocks.revealNode.mockImplementation((_spaceId: string, nodeId: string) =>
      nodeId === first.id ? firstReveal.promise : secondReveal.promise
    );
    const { result } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    const firstOpen = result.current.openNodeInNewGroup(first);
    const secondOpen = result.current.openNodeInNewGroup(second);
    await act(async () => {
      firstReveal.resolve({ ancestors: [], target: first });
      await firstOpen;
    });
    expect(useUiStore.getState().activeGroupIndex).toBe(1);
    await act(async () => {
      secondReveal.resolve({ ancestors: [], target: second });
      await secondOpen;
    });

    expect(useUiStore.getState().editorGroups.map((group) => group.node?.id)).toEqual([
      sourceNode.id,
      first.id,
      second.id
    ]);
    expect(useUiStore.getState().activeGroupIndex).toBe(2);
    expect(getNode).not.toHaveBeenCalled();
  });

  it("does not apply a pending open after the navigation hook unmounts", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const targetNode = node("target", activeSpace.id, "/folder/target.md");
    const folder = node("folder", activeSpace.id, "/folder", "folder");
    openSourceGroup(activeSpace, sourceNode);
    const pending = deferred<NodeRevealResponse>();
    mocks.revealNode.mockReturnValue(pending.promise);
    const { result, unmount } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    const opening = result.current.openNode(targetNode);
    unmount();
    const currentUi = navigationUiState();
    await act(async () => {
      pending.resolve({ ancestors: [folder], target: targetNode });
      expect(await opening).toBeNull();
    });

    expect(navigationUiState()).toEqual(currentUi);
  });

  it("reports a current open failure when reveal and the canonical fallback both fail", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    openSourceGroup(activeSpace, sourceNode);
    mocks.revealNode.mockRejectedValue(new ApiError("unavailable", 503));
    vi.mocked(getNode).mockRejectedValue(new ApiError("unavailable", 503));
    const { result } = renderNavigationActionsWithCanonicalLoader(activeSpace);

    await act(async () => {
      expect(await result.current.openNode(targetNode)).toBeNull();
    });

    expect(currentActiveNode()?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Could not open item");
  });

  it("opens an indexed link by node id through normal editor history", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    openSourceGroup(activeSpace, sourceNode);
    mocks.revealNode.mockResolvedValue({ ancestors: [], target: targetNode });
    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openNodeById(targetNode.id, sourceNode.id);
    });

    const group = useUiStore.getState().editorGroups[0];
    expect(mocks.revealNode).toHaveBeenCalledWith(activeSpace.id, targetNode.id);
    expect(group.node?.id).toBe(targetNode.id);
    expect(group.back.map((entry) => entry.nodeId)).toEqual([sourceNode.id]);
  });

  it("keeps the current editor when an indexed link target was deleted", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    openSourceGroup(activeSpace, sourceNode);
    mocks.revealNode.mockRejectedValue(new ApiError("not found", 404));
    const { result } = renderNavigationActions(activeSpace);

    await act(async () => {
      await result.current.openNodeById("deleted-node", sourceNode.id);
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
    expect(useUiStore.getState().toast).toBe("Link target not found");
  });

  it("does not open an indexed link after the originating editor group changes", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const replacementNode = node("replacement", activeSpace.id, "/replacement.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    openSourceGroup(activeSpace, sourceNode);
    const reveal = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    mocks.revealNode.mockReturnValue(reveal.promise);
    const { result } = renderNavigationActions(activeSpace);

    const opening = result.current.openNodeById(targetNode.id, sourceNode.id);
    act(() => useUiStore.getState().openInActiveGroup(replacementNode));
    await act(async () => {
      reveal.resolve({ ancestors: [], target: targetNode });
      await opening;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(replacementNode.id);
  });

  it("does not open an indexed link after the Inspector selection changes", async () => {
    const activeSpace = space("space-1");
    const sourceNode = node("source", activeSpace.id, "/source.md");
    const inspectedElsewhere = node("other", activeSpace.id, "/other.md");
    const targetNode = node("target", activeSpace.id, "/target.md");
    openSourceGroup(activeSpace, sourceNode);
    const reveal = deferred<{ ancestors: RestNode[]; target: RestNode }>();
    mocks.revealNode.mockReturnValue(reveal.promise);
    const { result, rerender } = renderNavigationActions(activeSpace);

    const opening = result.current.openNodeById(targetNode.id, sourceNode.id);
    rerender({ activeSpace, inspectedNode: inspectedElsewhere });
    await act(async () => {
      reveal.resolve({ ancestors: [], target: targetNode });
      await opening;
    });

    expect(useUiStore.getState().editorGroups[0].node?.id).toBe(sourceNode.id);
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
    expect(loadCanonicalNode).toHaveBeenCalledWith(rootNode, null);
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
  useUiStore.getState().setActiveSpaceId(activeSpace?.id ?? null);
  const queryClient = createTestQueryClient();
  const inspectedNode = currentActiveNode();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const rendered = renderHook(
    ({ activeSpace, inspectedNode }: { activeSpace: Space | null; inspectedNode: RestNode | null }) => useWorkbenchNodeNavigationActions({
      activeSpace,
      inspectedNode,
      loadCanonicalNode
    }),
    { wrapper, initialProps: { activeSpace, inspectedNode } }
  );
  return { ...rendered, queryClient };
}

function renderNavigationActionsWithCanonicalLoader(activeSpace: Space) {
  useUiStore.getState().setActiveSpaceId(activeSpace.id);
  const queryClient = createTestQueryClient();
  const inspectedNode = currentActiveNode();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const rendered = renderHook(({ activeSpace, inspectedNode }: { activeSpace: Space; inspectedNode: RestNode | null }) => {
    const loadCanonicalNode = useCanonicalNodeLoader();
    return useWorkbenchNodeNavigationActions({ activeSpace, inspectedNode, loadCanonicalNode });
  }, { wrapper, initialProps: { activeSpace, inspectedNode } });
  return { ...rendered, queryClient };
}

function currentActiveNode(): RestNode | null {
  const state = useUiStore.getState();
  return state.editorGroups[state.activeGroupIndex]?.node ?? null;
}

function navigationUiState() {
  const state = useUiStore.getState();
  return {
    activeSpaceId: state.activeSpaceId,
    editorGroups: state.editorGroups,
    activeGroupIndex: state.activeGroupIndex,
    expandedFolderIds: state.expandedFolderIds,
    mobileTreeOpen: state.mobileTreeOpen,
    mobileAuxOpen: state.mobileAuxOpen,
    toast: state.toast
  };
}

function openSourceGroup(activeSpace: Space, sourceNode: RestNode): number {
  useUiStore.getState().setActiveSpaceId(activeSpace.id);
  useUiStore.getState().openInActiveGroup(sourceNode);
  return useUiStore.getState().editorGroups[0].id;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
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
