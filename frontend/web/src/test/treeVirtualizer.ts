import { vi } from "vitest";

const TREE_ROW_SIZE = 36;

let visibleStart = 0;
let visibleLimit = Number.POSITIVE_INFINITY;

export const treeVirtualizerScrollToIndex = vi.fn();

export function resetTreeVirtualizer(limit = Number.POSITIVE_INFINITY) {
  visibleStart = 0;
  visibleLimit = limit;
  treeVirtualizerScrollToIndex.mockReset();
}

export function setTreeVirtualizerStart(index: number) {
  visibleStart = index;
}

export function defaultRangeExtractor({ startIndex, endIndex }: { startIndex: number; endIndex: number }) {
  return Array.from({ length: endIndex - startIndex + 1 }, (_, index) => startIndex + index);
}

export function useVirtualizer({ count, getItemKey }: {
  count: number;
  getItemKey: (index: number) => string | number;
}) {
  return {
    getTotalSize: () => count * TREE_ROW_SIZE,
    getVirtualItems: () => Array.from(
      { length: Math.min(Math.max(count - visibleStart, 0), visibleLimit) },
      (_, offset) => {
        const index = visibleStart + offset;
        return { index, key: getItemKey(index), size: TREE_ROW_SIZE, start: index * TREE_ROW_SIZE };
      }
    ),
    scrollToIndex: treeVirtualizerScrollToIndex
  };
}
