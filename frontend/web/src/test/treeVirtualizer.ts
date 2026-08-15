import type { Range } from "@tanstack/react-virtual";
import { vi, type Mock } from "vitest";

let visibleStart = 0;
let visibleLimit = Number.POSITIVE_INFINITY;

export const treeVirtualizerScrollToIndex: Mock = vi.fn();
export const treeVirtualizerMeasure: Mock = vi.fn();

export function resetTreeVirtualizer(limit = Number.POSITIVE_INFINITY) {
  visibleStart = 0;
  visibleLimit = limit;
  treeVirtualizerScrollToIndex.mockReset();
  treeVirtualizerMeasure.mockReset();
}

export function setTreeVirtualizerStart(index: number) {
  visibleStart = index;
}

export function defaultRangeExtractor({ startIndex, endIndex }: Range) {
  return Array.from({ length: endIndex - startIndex + 1 }, (_, index) => startIndex + index);
}

export function useVirtualizer({ count, estimateSize, getItemKey, rangeExtractor = defaultRangeExtractor }: {
  count: number;
  estimateSize: (index: number) => number;
  getItemKey: (index: number) => string | number;
  rangeExtractor?: (range: Range) => number[];
}) {
  const rowSize = estimateSize(0);
  return {
    getTotalSize: () => count * rowSize,
    getVirtualItems: () => {
      if (count <= visibleStart || visibleLimit <= 0) return [];
      const endIndex = Math.min(visibleStart + visibleLimit - 1, count - 1);
      return rangeExtractor({
        startIndex: visibleStart,
        endIndex,
        overscan: 0,
        count
      }).map((index) => ({
        index,
        key: getItemKey(index),
        size: rowSize,
        start: index * rowSize
      }));
    },
    scrollToIndex: treeVirtualizerScrollToIndex,
    measure: treeVirtualizerMeasure
  };
}
