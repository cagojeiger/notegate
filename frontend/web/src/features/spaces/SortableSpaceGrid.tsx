import {
  closestCenter,
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent
} from "@dnd-kit/core";
import {
  arrayMove,
  rectSortingStrategy,
  SortableContext,
  useSortable
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ArrowLeft, ArrowRight, GripVertical, Pin, PinOff } from "lucide-react";

import type { Space } from "../../api/types";
import type { SpaceUsage } from "../../api/usage";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Badge, Button, Card, IconButton } from "../../shared/ui";

type SortableSpaceGridProps = {
  spaces: Space[];
  selectedSpaceId: string | null;
  usageBySpaceId: Map<string, SpaceUsage>;
  pinPending: boolean;
  reorderPending: boolean;
  onSelect: (spaceId: string) => void;
  onOpen: (space: Space) => void;
  onTogglePin: (space: Space) => void;
  onReorder: (spaces: Space[]) => void;
};

export function SortableSpaceGrid({
  spaces,
  selectedSpaceId,
  usageBySpaceId,
  pinPending,
  reorderPending,
  onSelect,
  onOpen,
  onTogglePin,
  onReorder
}: SortableSpaceGridProps) {
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 8 } }));

  function moveSpace(spaceId: string, offset: -1 | 1) {
    const currentIndex = spaces.findIndex((space) => space.id === spaceId);
    const nextIndex = currentIndex + offset;
    if (currentIndex < 0 || nextIndex < 0 || nextIndex >= spaces.length) return;
    onReorder(arrayMove(spaces, currentIndex, nextIndex));
  }

  function handleDragEnd({ active, over }: DragEndEvent) {
    if (!over || active.id === over.id) return;
    const currentIndex = spaces.findIndex((space) => space.id === active.id);
    const nextIndex = spaces.findIndex((space) => space.id === over.id);
    if (currentIndex < 0 || nextIndex < 0) return;
    onReorder(arrayMove(spaces, currentIndex, nextIndex));
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext items={spaces.map((space) => space.id)} strategy={rectSortingStrategy}>
        <ul aria-label="All spaces" className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4">
          {spaces.map((space, index) => (
            <SortableSpaceCard
              key={space.id}
              space={space}
              usage={usageBySpaceId.get(space.id)}
              selected={selectedSpaceId === space.id}
              pinPending={pinPending}
              reorderPending={reorderPending}
              canMoveEarlier={index > 0}
              canMoveLater={index < spaces.length - 1}
              onSelect={() => onSelect(space.id)}
              onOpen={() => onOpen(space)}
              onTogglePin={() => onTogglePin(space)}
              onMoveEarlier={() => moveSpace(space.id, -1)}
              onMoveLater={() => moveSpace(space.id, 1)}
            />
          ))}
        </ul>
      </SortableContext>
    </DndContext>
  );
}

function SortableSpaceCard({
  space,
  usage,
  selected,
  pinPending,
  reorderPending,
  canMoveEarlier,
  canMoveLater,
  onSelect,
  onOpen,
  onTogglePin,
  onMoveEarlier,
  onMoveLater
}: {
  space: Space;
  usage: SpaceUsage | undefined;
  selected: boolean;
  pinPending: boolean;
  reorderPending: boolean;
  canMoveEarlier: boolean;
  canMoveLater: boolean;
  onSelect: () => void;
  onOpen: () => void;
  onTogglePin: () => void;
  onMoveEarlier: () => void;
  onMoveLater: () => void;
}) {
  const {
    attributes,
    listeners,
    setActivatorNodeRef,
    setNodeRef,
    transform,
    transition,
    isDragging
  } = useSortable({ id: space.id, disabled: reorderPending });

  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={isDragging ? "relative z-10 opacity-70" : ""}
    >
      <Card
        padding="none"
        className={[
          "h-full",
          selected ? "border-[var(--ng-active-border)] shadow-[var(--ng-inset-shadow)]" : "hover:border-border-strong",
          isDragging ? "shadow-[var(--ng-focus-shadow)]" : ""
        ].join(" ")}
      >
      <div className="flex items-center justify-between gap-3 px-3 pt-3">
        <span
          ref={setActivatorNodeRef}
          className={[
            "grid size-10 touch-none place-items-center rounded-xl text-muted transition hover:bg-[var(--ng-hover)] hover:text-text",
            reorderPending ? "cursor-not-allowed opacity-40" : "cursor-grab active:cursor-grabbing"
          ].join(" ")}
          {...attributes}
          {...listeners}
          role="presentation"
          tabIndex={-1}
          aria-hidden="true"
          data-testid={`drag-handle-${space.id}`}
        >
          <GripVertical size={17} />
        </span>
        <span className="grid size-10 place-items-center rounded-xl bg-panel-strong text-sm font-semibold text-text" aria-hidden="true">
          {space.name.slice(0, 1).toUpperCase()}
        </span>
        <span className="min-w-0 flex-1" />
        <Badge className={space.pinned ? "border-primary/40 text-primary" : undefined}>
          {space.pinned ? "Pinned" : "Unpinned"}
        </Badge>
      </div>

      <button
        type="button"
        className="block w-full px-4 pb-3 pt-3 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
        onClick={onSelect}
        aria-pressed={selected}
        aria-label={`Inspect ${space.name}`}
      >
        <span className="block truncate font-semibold">{space.name}</span>
        <span className="mt-1 block text-xs text-muted">
          {usage ? `${usage.items.used.toLocaleString()} items · ${formatBytes(usage.text_bytes.used + usage.file_bytes.used)}` : "Usage unavailable"}
        </span>
      </button>

      <div className="flex items-center justify-between gap-2 border-t border-seam px-3 py-2">
        <Button size="sm" variant="ghost" onClick={onOpen}>Open</Button>
        <div className="flex items-center gap-1">
          <IconButton
            label={`Move ${space.name} earlier`}
            size="sm"
            disabled={reorderPending || !canMoveEarlier}
            onClick={onMoveEarlier}
          >
            <ArrowLeft size={14} />
          </IconButton>
          <IconButton
            label={`Move ${space.name} later`}
            size="sm"
            disabled={reorderPending || !canMoveLater}
            onClick={onMoveLater}
          >
            <ArrowRight size={14} />
          </IconButton>
          <Button
            size="sm"
            variant="ghost"
            disabled={pinPending}
            onClick={onTogglePin}
            aria-label={`${space.pinned ? "Hide" : "Make"} ${space.name} ${space.pinned ? "from" : "available in"} user MCP`}
            title={space.pinned ? "Hide from user MCP" : "Make available in user MCP"}
          >
            {space.pinned ? <PinOff size={14} /> : <Pin size={14} />}
            {space.pinned ? "Unpin" : "Pin"}
          </Button>
        </div>
      </div>
      </Card>
    </li>
  );
}
