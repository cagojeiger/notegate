import type { Space } from "../../api/types";

const SORT_ORDER_STEP = 1000;

type DropPosition = "before" | "after";

export type SpaceSortOrderUpdate = {
  spaceId: string;
  sort_order: number;
};

export function reorderSpacesByDrop(spaces: Space[], draggedId: string, targetId: string, position: DropPosition): Space[] {
  const from = spaces.findIndex((space) => space.id === draggedId);
  const target = spaces.findIndex((space) => space.id === targetId);
  if (from < 0 || target < 0 || draggedId === targetId) return spaces;

  const next = [...spaces];
  const [dragged] = next.splice(from, 1);
  const targetAfterRemoval = next.findIndex((space) => space.id === targetId);
  if (targetAfterRemoval < 0) return spaces;

  const insertAt = position === "after" ? targetAfterRemoval + 1 : targetAfterRemoval;
  next.splice(insertAt, 0, dragged);
  return next;
}

export function buildSpaceSortOrderUpdates(spaces: Space[]): SpaceSortOrderUpdate[] {
  return spaces
    .map((space, index) => ({ spaceId: space.id, sort_order: (index + 1) * SORT_ORDER_STEP, current: space.sort_order }))
    .filter((update) => update.sort_order !== update.current)
    .map(({ spaceId, sort_order }) => ({ spaceId, sort_order }));
}
