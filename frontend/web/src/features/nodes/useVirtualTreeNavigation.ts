import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import type { FocusEvent as ReactFocusEvent, KeyboardEvent as ReactKeyboardEvent, RefObject } from "react";
import { useCallback, useEffect, useLayoutEffect, useState } from "react";

import { findAdjacentNodeRowIndex, type TreeRow } from "./treeProjection";
import type { TreeKeyboardNavigationRegistrar } from "./types";

const TREE_ROW_SIZE = 32;
const TREE_OVERSCAN = 8;

export function useVirtualTreeNavigation({
  rows,
  draggedNodeId,
  scrollRef,
  onTreeNavigationChange
}: {
  rows: readonly TreeRow[];
  draggedNodeId: string | null;
  scrollRef: RefObject<HTMLDivElement | null>;
  onTreeNavigationChange: TreeKeyboardNavigationRegistrar;
}) {
  const [focusedNodeId, setFocusedNodeId] = useState<string | null>(null);
  const [pendingFocusNodeId, setPendingFocusNodeId] = useState<string | null>(null);
  const getItemKey = useCallback(
    (index: number) => rows[index]?.key ?? index,
    [rows]
  );
  const draggedIndex = draggedNodeId
    ? rows.findIndex((row) => row.type === "node" && row.node.id === draggedNodeId)
    : -1;
  const focusedIndex = focusedNodeId
    ? rows.findIndex((row) => row.type === "node" && row.node.id === focusedNodeId)
    : -1;
  const rangeExtractor = useCallback((range: Range) => {
    const indexes = defaultRangeExtractor(range);
    for (const pinnedIndex of [draggedIndex, focusedIndex]) {
      if (pinnedIndex >= 0 && !indexes.includes(pinnedIndex)) indexes.push(pinnedIndex);
    }
    return indexes.sort((left, right) => left - right);
  }, [draggedIndex, focusedIndex]);
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => TREE_ROW_SIZE,
    getItemKey,
    overscan: TREE_OVERSCAN,
    rangeExtractor
  });
  const virtualItems = rowVirtualizer.getVirtualItems();
  const requestNodeFocus = useCallback((nodeId: string, index: number) => {
    setPendingFocusNodeId(nodeId);
    rowVirtualizer.scrollToIndex(index, { align: "auto" });
  }, [rowVirtualizer]);
  const focusLastNode = useCallback(() => {
    const index = findAdjacentNodeRowIndex(rows, rows.length, -1);
    const row = index === null ? undefined : rows[index];
    if (index === null || row?.type !== "node") return false;
    requestNodeFocus(row.node.id, index);
    return true;
  }, [requestNodeFocus, rows]);

  useLayoutEffect(() => {
    onTreeNavigationChange({ focusLastNode });
    return () => onTreeNavigationChange(null);
  }, [focusLastNode, onTreeNavigationChange]);

  useEffect(() => {
    if (pendingFocusNodeId === null) return;
    const pendingFocusIndex = rows.findIndex(
      (row) => row.type === "node" && row.node.id === pendingFocusNodeId
    );
    if (pendingFocusIndex < 0) {
      setPendingFocusNodeId(null);
      return;
    }
    const button = scrollRef.current?.querySelector<HTMLButtonElement>(
      `[data-tree-index="${pendingFocusIndex}"] [data-node-open]`
    );
    if (!button) {
      rowVirtualizer.scrollToIndex(pendingFocusIndex, { align: "auto" });
      return;
    }
    button.focus({ preventScroll: true });
    setPendingFocusNodeId(null);
  }, [pendingFocusNodeId, rowVirtualizer, rows, scrollRef, virtualItems]);

  function handleTreeKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const currentRow = (event.target as HTMLElement).closest("[data-tree-index]") as HTMLElement | null;
    const currentIndex = Number(currentRow?.dataset.treeIndex);
    if (!Number.isInteger(currentIndex)) return;
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const nextIndex = findAdjacentNodeRowIndex(rows, currentIndex, direction);
    if (nextIndex === null) return;
    const nextRow = rows[nextIndex];
    if (nextRow?.type !== "node") return;
    event.preventDefault();
    event.stopPropagation();
    requestNodeFocus(nextRow.node.id, nextIndex);
  }

  function handleTreeFocusCapture(event: ReactFocusEvent<HTMLDivElement>) {
    const row = (event.target as HTMLElement).closest("[data-tree-index]") as HTMLElement | null;
    const index = Number(row?.dataset.treeIndex);
    const treeRow = Number.isInteger(index) ? rows[index] : undefined;
    if (treeRow?.type === "node") setFocusedNodeId(treeRow.node.id);
  }

  function handleTreeBlurCapture(event: ReactFocusEvent<HTMLDivElement>) {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocusedNodeId(null);
  }

  return { rowVirtualizer, virtualItems, handleTreeKeyDown, handleTreeFocusCapture, handleTreeBlurCapture };
}
