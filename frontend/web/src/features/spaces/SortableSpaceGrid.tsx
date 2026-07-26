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
import {
  ArrowLeft,
  ArrowRight,
  Bot,
  BotOff,
  GripVertical,
  LockKeyhole,
  Pin,
  Search,
  SearchX,
  UnlockKeyhole
} from "lucide-react";
import { useState, type ReactNode } from "react";

import type { Space } from "../../api/types";
import type { SpaceUsage } from "../../api/usage";
import { formatBytes } from "../../shared/lib/formatBytes";
import { Button, Card, IconButton } from "../../shared/ui";

type SortableSpaceGridProps = {
  spaces: Space[];
  selectedSpaceId: string | null;
  usageBySpaceId: Map<string, SpaceUsage>;
  updatePending: boolean;
  reorderPending: boolean;
  onSelect: (spaceId: string) => void;
  onOpen: (space: Space) => void;
  onToggleNavigationPin: (space: Space) => void;
  onReorder: (spaces: Space[]) => void;
};

export function SortableSpaceGrid({
  spaces,
  selectedSpaceId,
  usageBySpaceId,
  updatePending,
  reorderPending,
  onSelect,
  onOpen,
  onToggleNavigationPin,
  onReorder
}: SortableSpaceGridProps) {
  const [reorderAnnouncement, setReorderAnnouncement] = useState("");
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 8 } }));

  function moveSpace(spaceId: string, offset: -1 | 1) {
    const currentIndex = spaces.findIndex((space) => space.id === spaceId);
    const nextIndex = currentIndex + offset;
    if (currentIndex < 0 || nextIndex < 0 || nextIndex >= spaces.length) return;
    onReorder(arrayMove(spaces, currentIndex, nextIndex));
    setReorderAnnouncement(
      `${spaces[currentIndex].name} moved to position ${nextIndex + 1} of ${spaces.length}`
    );
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
        <ul
          aria-label="All spaces"
          className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,16rem),1fr))] gap-3"
        >
          {spaces.map((space, index) => (
            <SortableSpaceCard
              key={space.id}
              space={space}
              usage={usageBySpaceId.get(space.id)}
              selected={selectedSpaceId === space.id}
              updatePending={updatePending}
              reorderPending={reorderPending}
              canMoveEarlier={index > 0}
              canMoveLater={index < spaces.length - 1}
              onSelect={() => onSelect(space.id)}
              onOpen={() => onOpen(space)}
              onToggleNavigationPin={() => onToggleNavigationPin(space)}
              onMoveEarlier={() => moveSpace(space.id, -1)}
              onMoveLater={() => moveSpace(space.id, 1)}
            />
          ))}
        </ul>
      </SortableContext>
      <p className="sr-only" role="status" aria-live="polite">
        {reorderAnnouncement}
      </p>
    </DndContext>
  );
}

function SortableSpaceCard({
  space,
  usage,
  selected,
  updatePending,
  reorderPending,
  canMoveEarlier,
  canMoveLater,
  onSelect,
  onOpen,
  onToggleNavigationPin,
  onMoveEarlier,
  onMoveLater
}: {
  space: Space;
  usage: SpaceUsage | undefined;
  selected: boolean;
  updatePending: boolean;
  reorderPending: boolean;
  canMoveEarlier: boolean;
  canMoveLater: boolean;
  onSelect: () => void;
  onOpen: () => void;
  onToggleNavigationPin: () => void;
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
          "h-full cursor-pointer transition has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-primary/45",
          selected
            ? "relative z-10 bg-[var(--ng-selection)] outline outline-1 -outline-offset-1 outline-[var(--ng-active-border)]"
            : "hover:border-border-strong",
          isDragging ? "shadow-[var(--ng-focus-shadow)]" : ""
        ].join(" ")}
        onClick={(event) => {
          if (
            event.target instanceof Element
            && event.target.closest("button, [data-space-drag-handle]")
          ) return;
          onSelect();
        }}
      >
        <div className="flex items-center gap-2 px-3 pt-3">
          <span
            ref={setActivatorNodeRef}
            className={[
              "grid size-8 touch-none place-items-center rounded-lg text-muted transition hover:bg-[var(--ng-hover)] hover:text-text",
              reorderPending ? "cursor-not-allowed opacity-40" : "cursor-grab active:cursor-grabbing"
            ].join(" ")}
            {...attributes}
            {...listeners}
            role="presentation"
            tabIndex={-1}
            aria-hidden="true"
            data-testid={`drag-handle-${space.id}`}
            data-space-drag-handle
          >
            <GripVertical size={16} />
          </span>
          <span
            className="grid size-9 shrink-0 place-items-center rounded-lg bg-panel-strong text-sm font-semibold text-text"
            aria-hidden="true"
          >
            {space.name.slice(0, 1).toUpperCase()}
          </span>
          <button
            type="button"
            className="min-w-0 flex-1 py-1 text-left outline-none"
            onClick={onSelect}
            aria-pressed={selected}
            aria-label={`Inspect ${space.name}`}
          >
            <span className="block truncate font-semibold">{space.name}</span>
            <span className="mt-0.5 block truncate text-xs text-muted">
              {usage
                ? `${usage.items.used.toLocaleString()} items · ${formatBytes(usage.text_bytes.used + usage.file_bytes.used)}`
                : "Usage unavailable"}
            </span>
          </button>
          <IconButton
            label={`${space.navigation_pinned ? "Unpin" : "Pin"} ${space.name} ${space.navigation_pinned ? "from" : "to"} navigation`}
            size="sm"
            pressed={space.navigation_pinned}
            disabled={updatePending}
            onClick={onToggleNavigationPin}
          >
            <Pin size={14} />
          </IconButton>
        </div>

        <div className="flex items-center gap-1 px-4 pb-3 pt-2">
          <StatusItem
            description={`Search default ${space.default_search_enabled ? "on" : "off"}`}
            active={space.default_search_enabled}
          >
            {space.default_search_enabled ? <Search size={15} /> : <SearchX size={15} />}
          </StatusItem>
          <StatusItem
            description={`User MCP access ${space.user_mcp_enabled ? "on" : "off"}`}
            active={space.user_mcp_enabled}
          >
            {space.user_mcp_enabled ? <Bot size={15} /> : <BotOff size={15} />}
          </StatusItem>
          <StatusItem
            description={`Default text encryption ${space.default_text_encryption_enabled ? "on" : "off"}`}
            active={space.default_text_encryption_enabled}
          >
            {space.default_text_encryption_enabled
              ? <LockKeyhole size={15} />
              : <UnlockKeyhole size={15} />}
          </StatusItem>
        </div>

        <div className="flex items-center justify-between gap-2 px-3 pb-3 pt-1">
          <Button size="sm" variant="ghost" onClick={onOpen}>
            Open
          </Button>
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
          </div>
        </div>
      </Card>
    </li>
  );
}

function StatusItem({
  description,
  active,
  children
}: {
  description: string;
  active: boolean;
  children: ReactNode;
}) {
  return (
    <span
      title={description}
      className={[
        "grid size-7 place-items-center rounded-md",
        active
          ? "bg-primary/10 text-primary"
          : "bg-panel text-muted"
      ].join(" ")}
    >
      <span aria-hidden="true">{children}</span>
      <span className="sr-only">{description}</span>
    </span>
  );
}
