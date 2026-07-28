import { act, renderHook, waitFor } from "@testing-library/react";
import type { FocusEvent as ReactFocusEvent } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { makeNodeSummary } from "../../test/fixtures";
import {
  resetTreeVirtualizer,
  treeVirtualizerScrollToIndex
} from "../../test/treeVirtualizer";
import type { TreeRow } from "./treeProjection";
import type { TreeKeyboardNavigation } from "./types";
import { useVirtualTreeNavigation } from "./useVirtualTreeNavigation";

vi.mock("@tanstack/react-virtual", () => import("../../test/treeVirtualizer"));

describe("useVirtualTreeNavigation", () => {
  beforeEach(() => {
    resetTreeVirtualizer(2);
  });

  it("keeps dragged and focused nodes inside the virtual range", () => {
    const rows = nodeRows(30);
    const tree = document.body.appendChild(document.createElement("div"));
    const focusedButton = appendTreeButton(tree, 28, "file-28.bin");
    const { result } = renderHook(() =>
      useVirtualTreeNavigation({
        rows,
        draggedNodeId: "file-29",
        scrollRef: { current: tree },
        onTreeNavigationChange: vi.fn()
      })
    );

    expect(virtualIndexes(result.current)).toEqual([0, 1, 29]);
    act(() =>
      result.current.handleTreeFocusCapture({
        target: focusedButton
      } as unknown as ReactFocusEvent<HTMLDivElement>)
    );
    expect(virtualIndexes(result.current)).toEqual([0, 1, 28, 29]);
    act(() =>
      result.current.handleTreeBlurCapture({
        currentTarget: tree,
        relatedTarget: document.body
      } as unknown as ReactFocusEvent<HTMLDivElement>)
    );
    expect(virtualIndexes(result.current)).toEqual([0, 1, 29]);

    tree.remove();
  });

  it("resolves pending focus by node id and unregisters navigation on unmount", async () => {
    resetTreeVirtualizer(20);
    const rows = nodeRows(30);
    const tree = document.body.appendChild(document.createElement("div"));
    const scrollRef = { current: tree };
    let navigation: TreeKeyboardNavigation | null = null;
    const onTreeNavigationChange = vi.fn((next: TreeKeyboardNavigation | null) => {
      navigation = next;
    });
    const view = renderHook(
      ({ currentRows }) =>
        useVirtualTreeNavigation({
          rows: currentRows,
          draggedNodeId: null,
          scrollRef,
          onTreeNavigationChange
        }),
      { initialProps: { currentRows: rows } }
    );

    await waitFor(() => expect(navigation).not.toBeNull());
    act(() => expect(navigation?.focusLastNode()).toBe(true));
    expect(treeVirtualizerScrollToIndex).toHaveBeenCalledWith(29, { align: "auto" });

    const targetButton = appendTreeButton(tree, 30, "file-29.bin");
    view.rerender({ currentRows: [treeRow("inserted"), ...rows] });

    await waitFor(() => expect(targetButton).toHaveFocus());

    view.unmount();
    expect(onTreeNavigationChange).toHaveBeenLastCalledWith(null);
    tree.remove();
  });
});

function nodeRows(count: number): TreeRow[] {
  return Array.from({ length: count }, (_, index) => treeRow(`file-${index}`));
}

function treeRow(id: string): TreeRow {
  const name = `${id}.bin`;
  return {
    type: "node",
    key: `node:${id}`,
    node: makeNodeSummary({ id, name, kind: "file", path: `/${name}` }),
    depth: 0
  };
}

function appendTreeButton(tree: HTMLDivElement, index: number, name: string) {
  const row = document.createElement("div");
  row.dataset.treeIndex = String(index);
  const button = document.createElement("button");
  button.dataset.nodeOpen = "";
  button.textContent = name;
  row.appendChild(button);
  tree.appendChild(row);
  return button;
}

function virtualIndexes(result: ReturnType<typeof useVirtualTreeNavigation>) {
  return result.virtualItems.map(({ index }) => index);
}
